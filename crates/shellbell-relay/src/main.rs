use anyhow::{Context, bail};
use axum::{
    Json,
    body::to_bytes,
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use chrono::{Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use shellbell_core::{hash_password, hash_secret, random_token, verify_password, verify_secret};
use shellbell_protocol::{ApiError, SessionResponse};
use shellbell_relay::{RelayConfig, build_app, open_database, push::VapidPushDelivery};
use sqlx::SqlitePool;
use std::{
    collections::HashMap,
    env, fs,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

const PASSWORD_MIN_CHARS: usize = 12;
const PASSWORD_MAX_CHARS: usize = 128;
const OWNER_ATTEMPT_LIMIT: u32 = 10;
const OWNER_ATTEMPT_WINDOW: Duration = Duration::from_secs(5 * 60);
const OWNER_PASSWORD_SETTING: &str = "owner_password_hash";

#[derive(Clone)]
struct OwnerPasswordState {
    pool: SqlitePool,
    bootstrap_token_hash: String,
    secure_cookies: bool,
    session_ttl: ChronoDuration,
    attempts: Arc<Mutex<HashMap<String, (Instant, u32)>>>,
}

#[derive(Debug, Serialize)]
struct OwnerBootstrapStatus {
    bootstrap_required: bool,
    password_required: bool,
}

#[derive(Debug, Deserialize)]
struct OwnerPasswordRequest {
    bootstrap_token: String,
    password: String,
    #[serde(default)]
    reset: bool,
}

#[derive(Debug, Deserialize)]
struct OwnerLoginRequest {
    password: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("shellbell_relay=info,tower_http=info")),
        )
        .json()
        .init();
    let data_dir = PathBuf::from(env::var("SHELLBELL_DATA_DIR").unwrap_or_else(|_| "/data".into()));
    fs::create_dir_all(&data_dir).with_context(|| format!("create {}", data_dir.display()))?;
    secure_dir(&data_dir)?;
    let bootstrap = load_or_create_bootstrap(&data_dir)?;
    let bootstrap_token_hash = hash_secret(&bootstrap);
    let database_url = env::var("SHELLBELL_DATABASE_URL")
        .unwrap_or_else(|_| format!("sqlite://{}/shellbell.db", data_dir.display()));
    let secure_cookies = env::var("SHELLBELL_INSECURE_LOCAL_HTTP").as_deref() != Ok("true");
    let vapid_private = env::var("SHELLBELL_VAPID_PRIVATE_KEY")
        .context("SHELLBELL_VAPID_PRIVATE_KEY is required")?;
    let vapid_public =
        env::var("SHELLBELL_VAPID_PUBLIC_KEY").context("SHELLBELL_VAPID_PUBLIC_KEY is required")?;
    let vapid_subject =
        env::var("SHELLBELL_VAPID_SUBJECT").unwrap_or_else(|_| "mailto:owner@localhost".into());
    let retention = env::var("SHELLBELL_HISTORY_RETENTION_DAYS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(14);
    let session_days = match env::var("SHELLBELL_OWNER_SESSION_DAYS") {
        Ok(value) => value
            .parse::<i64>()
            .context("SHELLBELL_OWNER_SESSION_DAYS must be an integer")?,
        Err(_) => 30,
    };
    if !(1..=90).contains(&session_days) {
        bail!("SHELLBELL_OWNER_SESSION_DAYS must be between 1 and 90");
    }
    let static_dir = env::var_os("SHELLBELL_STATIC_DIR")
        .map(PathBuf::from)
        .or_else(|| Some(PathBuf::from("/app/pwa")));
    let config = RelayConfig {
        bootstrap_token_hash: bootstrap_token_hash.clone(),
        secure_cookies,
        session_ttl: ChronoDuration::days(session_days),
        pairing_ttl: ChronoDuration::minutes(10),
        default_retention_days: retention,
        vapid_public_key: vapid_public,
        static_dir,
    };
    let pool = open_database(&database_url).await?;
    let owner_state = OwnerPasswordState {
        pool: pool.clone(),
        bootstrap_token_hash,
        secure_cookies,
        session_ttl: ChronoDuration::days(session_days),
        attempts: Arc::new(Mutex::new(HashMap::new())),
    };
    let app = build_app(
        pool,
        config,
        Arc::new(VapidPushDelivery::new(
            vapid_private,
            vapid_subject,
            Duration::from_secs(8),
        )),
    )
    .await?
    .layer(middleware::from_fn_with_state(
        owner_state,
        owner_password_middleware,
    ));
    let address: SocketAddr = env::var("SHELLBELL_LISTEN")
        .unwrap_or_else(|_| "0.0.0.0:8080".into())
        .parse()?;
    tracing::info!(%address, bootstrap_file = %data_dir.join("owner-bootstrap-token").display(), "relay listening; bootstrap secret is never logged");
    axum::serve(TcpListener::bind(address).await?, app.into_make_service()).await?;
    Ok(())
}

async fn owner_password_middleware(
    State(state): State<OwnerPasswordState>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path().to_owned();
    let method = request.method().clone();

    if path == "/api/owner/bootstrap/status" && method == Method::GET {
        let bootstrapped_at: Option<String> =
            match sqlx::query_scalar("SELECT bootstrapped_at FROM owner_state WHERE singleton=1")
                .fetch_one(&state.pool)
                .await
            {
                Ok(value) => value,
                Err(error) => return database_error(error),
            };
        let password_hash: Option<String> =
            match sqlx::query_scalar("SELECT value FROM settings WHERE key=?")
                .bind(OWNER_PASSWORD_SETTING)
                .fetch_optional(&state.pool)
                .await
            {
                Ok(value) => value,
                Err(error) => return database_error(error),
            };
        return Json(OwnerBootstrapStatus {
            bootstrap_required: bootstrapped_at.is_none(),
            password_required: password_hash.is_none(),
        })
        .into_response();
    }

    if path == "/api/owner/bootstrap" && method == Method::POST {
        let (headers, input) = match read_json::<OwnerPasswordRequest>(request).await {
            Ok(value) => value,
            Err(response) => return response,
        };
        if !allow_owner_attempt(&state, &headers, "bootstrap") {
            return api_error(
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "too many attempts; retry later",
            );
        }
        if !verify_secret(&input.bootstrap_token, &state.bootstrap_token_hash) {
            return api_error(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "authentication required",
            );
        }
        if let Err(message) = validate_owner_password(&input.password) {
            return api_error(StatusCode::BAD_REQUEST, "validation_error", message);
        }

        let bootstrapped_at: Option<String> =
            match sqlx::query_scalar("SELECT bootstrapped_at FROM owner_state WHERE singleton=1")
                .fetch_one(&state.pool)
                .await
            {
                Ok(value) => value,
                Err(error) => return database_error(error),
            };
        let existing_password_hash: Option<String> =
            match sqlx::query_scalar("SELECT value FROM settings WHERE key=?")
                .bind(OWNER_PASSWORD_SETTING)
                .fetch_optional(&state.pool)
                .await
            {
                Ok(value) => value,
                Err(error) => return database_error(error),
            };
        if bootstrapped_at.is_some() && existing_password_hash.is_some() && !input.reset {
            return api_error(
                StatusCode::CONFLICT,
                "owner_already_configured",
                "owner password is already configured",
            );
        }

        let now = Utc::now();
        let mut tx = match state.pool.begin().await {
            Ok(value) => value,
            Err(error) => return database_error(error),
        };
        if bootstrapped_at.is_none()
            && let Err(error) = sqlx::query(
                "UPDATE owner_state SET bootstrapped_at=? WHERE singleton=1 AND bootstrapped_at IS NULL",
            )
            .bind(now)
            .execute(&mut *tx)
            .await
        {
            return database_error(error);
        }
        let password_hash = hash_password(&input.password);
        if let Err(error) = sqlx::query(
            "INSERT INTO settings(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        )
        .bind(OWNER_PASSWORD_SETTING)
        .bind(password_hash)
        .execute(&mut *tx)
        .await
        {
            return database_error(error);
        }
        if let Err(error) =
            sqlx::query("UPDATE owner_sessions SET revoked_at=? WHERE revoked_at IS NULL")
                .bind(now)
                .execute(&mut *tx)
                .await
        {
            return database_error(error);
        }
        if let Err(error) = tx.commit().await {
            return database_error(error);
        }
        return match issue_owner_session(&state).await {
            Ok(response) => response,
            Err(response) => response,
        };
    }

    if path == "/api/owner/session" && method == Method::POST {
        let (headers, input) = match read_json::<OwnerLoginRequest>(request).await {
            Ok(value) => value,
            Err(response) => return response,
        };
        if !allow_owner_attempt(&state, &headers, "login") {
            return api_error(
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "too many attempts; retry later",
            );
        }
        let stored_hash: Option<String> =
            match sqlx::query_scalar("SELECT value FROM settings WHERE key=?")
                .bind(OWNER_PASSWORD_SETTING)
                .fetch_optional(&state.pool)
                .await
            {
                Ok(value) => value,
                Err(error) => return database_error(error),
            };
        let Some(stored_hash) = stored_hash else {
            return api_error(
                StatusCode::CONFLICT,
                "password_required",
                "set an owner password with the bootstrap token first",
            );
        };
        if !verify_password(&input.password, &stored_hash) {
            return api_error(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "authentication required",
            );
        }
        return match issue_owner_session(&state).await {
            Ok(response) => response,
            Err(response) => response,
        };
    }

    next.run(request).await
}

async fn read_json<T: DeserializeOwned>(request: Request) -> Result<(HeaderMap, T), Response> {
    let (parts, body) = request.into_parts();
    let bytes = to_bytes(body, 16 * 1024).await.map_err(|_| {
        api_error(
            StatusCode::BAD_REQUEST,
            "validation_error",
            "invalid request body",
        )
    })?;
    let value = serde_json::from_slice(&bytes).map_err(|_| {
        api_error(
            StatusCode::BAD_REQUEST,
            "validation_error",
            "invalid JSON body",
        )
    })?;
    Ok((parts.headers, value))
}

fn validate_owner_password(password: &str) -> Result<(), &'static str> {
    let length = password.chars().count();
    if length < PASSWORD_MIN_CHARS {
        return Err("owner password must be at least 12 characters");
    }
    if length > PASSWORD_MAX_CHARS {
        return Err("owner password must be at most 128 characters");
    }
    if password.chars().any(char::is_control) {
        return Err("owner password must not contain control characters");
    }
    Ok(())
}

async fn issue_owner_session(state: &OwnerPasswordState) -> Result<Response, Response> {
    let token = random_token("sb_session_");
    let csrf = random_token("sb_csrf_");
    let now = Utc::now();
    let expires = now + state.session_ttl;
    sqlx::query("INSERT INTO owner_sessions(id,token_hash,created_at,expires_at) VALUES(?,?,?,?)")
        .bind(Uuid::new_v4().to_string())
        .bind(hash_secret(&token))
        .bind(now)
        .bind(expires)
        .execute(&state.pool)
        .await
        .map_err(database_error)?;
    let secure = if state.secure_cookies { "; Secure" } else { "" };
    let max_age = state.session_ttl.num_seconds();
    let mut headers = HeaderMap::new();
    headers.append(
        header::SET_COOKIE,
        HeaderValue::from_str(&format!(
            "sb_session={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}{secure}"
        ))
        .map_err(|_| internal_error("could not issue owner session cookie"))?,
    );
    headers.append(
        header::SET_COOKIE,
        HeaderValue::from_str(&format!(
            "sb_csrf={csrf}; Path=/; SameSite=Strict; Max-Age={max_age}{secure}"
        ))
        .map_err(|_| internal_error("could not issue CSRF cookie"))?,
    );
    Ok((
        headers,
        Json(SessionResponse {
            authenticated: true,
            expires_at: expires,
        }),
    )
        .into_response())
}

fn allow_owner_attempt(state: &OwnerPasswordState, headers: &HeaderMap, action: &str) -> bool {
    let client = headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .unwrap_or("local")
        .trim();
    let key = format!("{action}:{client}");
    let mut attempts = state.attempts.lock().expect("owner attempt limiter lock");
    let now = Instant::now();
    let entry = attempts.entry(key).or_insert((now, 0));
    if now.duration_since(entry.0) >= OWNER_ATTEMPT_WINDOW {
        *entry = (now, 0);
    }
    if entry.1 >= OWNER_ATTEMPT_LIMIT {
        return false;
    }
    entry.1 += 1;
    true
}

fn api_error(status: StatusCode, code: &'static str, message: impl Into<String>) -> Response {
    (status, Json(ApiError::new(code, message))).into_response()
}

fn database_error(error: sqlx::Error) -> Response {
    tracing::error!(error = %error, "database operation failed");
    internal_error("internal server error")
}

fn internal_error(message: &'static str) -> Response {
    api_error(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", message)
}

fn load_or_create_bootstrap(data_dir: &Path) -> anyhow::Result<String> {
    if let Ok(value) = env::var("SHELLBELL_OWNER_BOOTSTRAP_TOKEN") {
        if value.len() < 32 {
            bail!("SHELLBELL_OWNER_BOOTSTRAP_TOKEN must be at least 32 characters");
        }
        return Ok(value);
    }
    let path = data_dir.join("owner-bootstrap-token");
    if path.exists() {
        return Ok(fs::read_to_string(path)?.trim().to_owned());
    }
    let token = random_token("sb_owner_");
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        writeln!(file, "{token}")?;
    }
    #[cfg(not(unix))]
    fs::write(&path, format!("{token}\n"))?;
    Ok(token)
}

fn secure_dir(path: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
