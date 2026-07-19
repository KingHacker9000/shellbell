pub mod push;

use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use push::{DeliveryOutcome, PushDelivery, PushSubscription};
use serde::{Deserialize, Serialize};
use shellbell_core::{hash_secret, pairing_code, random_token, verify_secret};
use shellbell_protocol::*;
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::{
    collections::HashMap,
    path::PathBuf,
    str::FromStr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tower_http::{
    compression::CompressionLayer,
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};
use uuid::Uuid;

#[derive(Clone)]
pub struct RelayConfig {
    pub bootstrap_token_hash: String,
    pub secure_cookies: bool,
    pub session_ttl: ChronoDuration,
    pub pairing_ttl: ChronoDuration,
    pub default_retention_days: u32,
    pub vapid_public_key: String,
    pub static_dir: Option<PathBuf>,
}

impl RelayConfig {
    pub fn development(bootstrap_token: &str) -> Self {
        Self {
            bootstrap_token_hash: hash_secret(bootstrap_token),
            secure_cookies: false,
            session_ttl: ChronoDuration::hours(24),
            pairing_ttl: ChronoDuration::minutes(10),
            default_retention_days: 14,
            vapid_public_key: "development-vapid-public-key".into(),
            static_dir: None,
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub config: RelayConfig,
    pub push: Arc<dyn PushDelivery>,
    limiter: Arc<RateLimiter>,
    ring_lock: Arc<tokio::sync::Mutex<()>>,
}

#[derive(Default)]
struct RateLimiter {
    entries: Mutex<HashMap<String, (Instant, u32)>>,
}

impl RateLimiter {
    fn check(&self, key: String, limit: u32, window: Duration) -> bool {
        let mut entries = self.entries.lock().expect("rate limiter lock");
        let now = Instant::now();
        let entry = entries.entry(key).or_insert((now, 0));
        if now.duration_since(entry.0) >= window {
            *entry = (now, 0);
        }
        if entry.1 >= limit {
            return false;
        }
        entry.1 += 1;
        true
    }
}

#[derive(Debug)]
pub struct AppError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl AppError {
    fn bad(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: "validation_error",
            message: message.into(),
        }
    }
    fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message: "authentication required".into(),
        }
    }
    fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            code: "forbidden",
            message: message.into(),
        }
    }
    fn not_found() -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            code: "not_found",
            message: "resource not found".into(),
        }
    }
    fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            code: "conflict",
            message: message.into(),
        }
    }
    fn rate_limited() -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            code: "rate_limited",
            message: "too many requests; retry later".into(),
        }
    }
    fn db(error: sqlx::Error) -> Self {
        tracing::error!(error = %error, "database operation failed");
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            code: "internal_error",
            message: "internal server error".into(),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (self.status, Json(ApiError::new(self.code, self.message))).into_response()
    }
}

pub async fn open_database(url: &str) -> anyhow::Result<SqlitePool> {
    let options = SqliteConnectOptions::from_str(url)?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);
    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(options)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

pub async fn build_app(
    pool: SqlitePool,
    config: RelayConfig,
    push: Arc<dyn PushDelivery>,
) -> anyhow::Result<Router> {
    sqlx::query("INSERT INTO settings(key,value) VALUES('history_retention_days',?) ON CONFLICT(key) DO NOTHING").bind(config.default_retention_days.to_string()).execute(&pool).await?;
    let static_dir = config.static_dir.clone();
    let state = AppState {
        pool,
        config,
        push,
        limiter: Arc::new(RateLimiter::default()),
        ring_lock: Arc::new(tokio::sync::Mutex::new(())),
    };
    let api = Router::new()
        .route("/health", get(health))
        .route("/api/owner/bootstrap/status", get(bootstrap_status))
        .route("/api/owner/bootstrap", post(bootstrap))
        .route(
            "/api/owner/session",
            post(create_session).get(validate_session),
        )
        .route("/api/owner/logout", post(logout))
        .route("/api/pairings", post(create_pairing).get(list_pairings))
        .route("/api/pairings/{id}", get(poll_pairing))
        .route("/api/pairings/{id}/approve", post(approve_pairing))
        .route("/api/pairings/{id}/reject", post(reject_pairing))
        .route("/api/sources", get(list_sources))
        .route("/api/sources/self", get(source_self))
        .route(
            "/api/sources/{id}",
            patch(rename_source).delete(revoke_source),
        )
        .route("/api/receivers", post(create_receiver).get(list_receivers))
        .route(
            "/api/receivers/{id}",
            patch(update_receiver).delete(revoke_receiver),
        )
        .route("/api/rings", post(submit_ring).get(list_rings))
        .route("/api/settings", get(get_settings).put(update_settings))
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        .with_state(state);
    Ok(if let Some(dir) = static_dir {
        api.fallback_service(
            ServeDir::new(&dir).not_found_service(ServeFile::new(dir.join("index.html"))),
        )
    } else {
        api
    })
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    })
}

async fn bootstrap_status(
    State(state): State<AppState>,
) -> Result<Json<BootstrapStatusResponse>, AppError> {
    let value: Option<String> =
        sqlx::query_scalar("SELECT bootstrapped_at FROM owner_state WHERE singleton=1")
            .fetch_one(&state.pool)
            .await
            .map_err(AppError::db)?;
    Ok(Json(BootstrapStatusResponse {
        bootstrap_required: value.is_none(),
    }))
}

async fn bootstrap(
    State(state): State<AppState>,
    Json(request): Json<BootstrapRequest>,
) -> Result<(HeaderMap, Json<SessionResponse>), AppError> {
    if !verify_secret(&request.bootstrap_token, &state.config.bootstrap_token_hash) {
        return Err(AppError::unauthorized());
    }
    let mut tx = state.pool.begin().await.map_err(AppError::db)?;
    let existing: Option<String> =
        sqlx::query_scalar("SELECT bootstrapped_at FROM owner_state WHERE singleton=1")
            .fetch_one(&mut *tx)
            .await
            .map_err(AppError::db)?;
    if existing.is_some() {
        return Err(AppError::conflict("owner is already bootstrapped"));
    }
    sqlx::query(
        "UPDATE owner_state SET bootstrapped_at=? WHERE singleton=1 AND bootstrapped_at IS NULL",
    )
    .bind(Utc::now())
    .execute(&mut *tx)
    .await
    .map_err(AppError::db)?;
    let (headers, response) = issue_session(&state, &mut tx).await?;
    tx.commit().await.map_err(AppError::db)?;
    Ok((headers, Json(response)))
}

async fn create_session(
    State(state): State<AppState>,
    Json(request): Json<BootstrapRequest>,
) -> Result<(HeaderMap, Json<SessionResponse>), AppError> {
    if !verify_secret(&request.bootstrap_token, &state.config.bootstrap_token_hash) {
        return Err(AppError::unauthorized());
    }
    let bootstrapped: Option<String> =
        sqlx::query_scalar("SELECT bootstrapped_at FROM owner_state WHERE singleton=1")
            .fetch_one(&state.pool)
            .await
            .map_err(AppError::db)?;
    if bootstrapped.is_none() {
        return Err(AppError::conflict("owner bootstrap is required"));
    }
    let mut tx = state.pool.begin().await.map_err(AppError::db)?;
    let (headers, response) = issue_session(&state, &mut tx).await?;
    tx.commit().await.map_err(AppError::db)?;
    Ok((headers, Json(response)))
}

async fn issue_session(
    state: &AppState,
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
) -> Result<(HeaderMap, SessionResponse), AppError> {
    let token = random_token("sb_session_");
    let csrf = random_token("sb_csrf_");
    let now = Utc::now();
    let expires = now + state.config.session_ttl;
    sqlx::query("INSERT INTO owner_sessions(id,token_hash,created_at,expires_at) VALUES(?,?,?,?)")
        .bind(Uuid::new_v4().to_string())
        .bind(hash_secret(&token))
        .bind(now)
        .bind(expires)
        .execute(&mut **tx)
        .await
        .map_err(AppError::db)?;
    let secure = if state.config.secure_cookies {
        "; Secure"
    } else {
        ""
    };
    let max_age = state.config.session_ttl.num_seconds();
    let mut headers = HeaderMap::new();
    headers.append(
        header::SET_COOKIE,
        HeaderValue::from_str(&format!(
            "sb_session={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}{secure}"
        ))
        .expect("valid cookie"),
    );
    headers.append(
        header::SET_COOKIE,
        HeaderValue::from_str(&format!(
            "sb_csrf={csrf}; Path=/; SameSite=Strict; Max-Age={max_age}{secure}"
        ))
        .expect("valid cookie"),
    );
    Ok((
        headers,
        SessionResponse {
            authenticated: true,
            expires_at: expires,
        },
    ))
}

async fn owner_auth(
    state: &AppState,
    headers: &HeaderMap,
    mutation: bool,
) -> Result<String, AppError> {
    let session = cookie_value(headers, "sb_session").ok_or_else(AppError::unauthorized)?;
    if mutation {
        let csrf_cookie = cookie_value(headers, "sb_csrf")
            .ok_or_else(|| AppError::forbidden("CSRF token missing"))?;
        let csrf_header = headers
            .get("x-csrf-token")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| AppError::forbidden("CSRF token missing"))?;
        if !verify_secret(csrf_header, &hash_secret(&csrf_cookie)) {
            return Err(AppError::forbidden("CSRF token mismatch"));
        }
    }
    let rows = sqlx::query(
        "SELECT id,token_hash FROM owner_sessions WHERE revoked_at IS NULL AND expires_at > ?",
    )
    .bind(Utc::now())
    .fetch_all(&state.pool)
    .await
    .map_err(AppError::db)?;
    for row in rows {
        let hash: String = row.get("token_hash");
        if verify_secret(&session, &hash) {
            return Ok(row.get("id"));
        }
    }
    Err(AppError::unauthorized())
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|part| part.trim().split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value.to_owned())
}

fn clear_cookies(secure: bool) -> HeaderMap {
    let secure = if secure { "; Secure" } else { "" };
    let mut headers = HeaderMap::new();
    headers.append(
        header::SET_COOKIE,
        HeaderValue::from_str(&format!(
            "sb_session=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0{secure}"
        ))
        .unwrap(),
    );
    headers.append(
        header::SET_COOKIE,
        HeaderValue::from_str(&format!(
            "sb_csrf=; Path=/; SameSite=Strict; Max-Age=0{secure}"
        ))
        .unwrap(),
    );
    headers
}

async fn validate_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SessionResponse>, AppError> {
    let id = owner_auth(&state, &headers, false).await?;
    let expires: DateTime<Utc> =
        sqlx::query_scalar("SELECT expires_at FROM owner_sessions WHERE id=?")
            .bind(id)
            .fetch_one(&state.pool)
            .await
            .map_err(AppError::db)?;
    Ok(Json(SessionResponse {
        authenticated: true,
        expires_at: expires,
    }))
}

async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(HeaderMap, StatusCode), AppError> {
    let id = owner_auth(&state, &headers, true).await?;
    sqlx::query("UPDATE owner_sessions SET revoked_at=? WHERE id=?")
        .bind(Utc::now())
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(AppError::db)?;
    Ok((
        clear_cookies(state.config.secure_cookies),
        StatusCode::NO_CONTENT,
    ))
}

fn rate_key(headers: &HeaderMap, action: &str) -> String {
    let client = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .unwrap_or("local");
    format!("{action}:{client}")
}

async fn create_pairing(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<PairingCreateRequest>,
) -> Result<(StatusCode, Json<PairingCreateResponse>), AppError> {
    if !state
        .limiter
        .check(rate_key(&headers, "pair"), 12, Duration::from_secs(60))
    {
        return Err(AppError::rate_limited());
    }
    let display_name = validate_name(&request.display_name).map_err(AppError::bad)?;
    let id = Uuid::new_v4();
    let code = pairing_code();
    let now = Utc::now();
    let expires = now + state.config.pairing_ttl;
    sqlx::query("INSERT INTO pairing_requests(id,code,display_name,status,created_at,expires_at) VALUES(?,?,?,'pending',?,?)")
        .bind(id.to_string()).bind(&code).bind(display_name).bind(now).bind(expires).execute(&state.pool).await.map_err(AppError::db)?;
    Ok((
        StatusCode::CREATED,
        Json(PairingCreateResponse {
            id,
            code,
            expires_at: expires,
        }),
    ))
}

async fn poll_pairing(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<PairingPollResponse>, AppError> {
    let row = sqlx::query("SELECT status,expires_at,source_id FROM pairing_requests WHERE id=?")
        .bind(id.to_string())
        .fetch_optional(&state.pool)
        .await
        .map_err(AppError::db)?
        .ok_or_else(AppError::not_found)?;
    let mut status: String = row.get("status");
    let expires: DateTime<Utc> = row.get("expires_at");
    let source_id: Option<String> = row.get("source_id");
    if status == "pending" && expires <= Utc::now() {
        status = "expired".into();
        sqlx::query("UPDATE pairing_requests SET status='expired',decided_at=? WHERE id=? AND status='pending'").bind(Utc::now()).bind(id.to_string()).execute(&state.pool).await.map_err(AppError::db)?;
    }
    let mut token = None;
    if status == "approved" {
        let source = source_id
            .as_ref()
            .ok_or_else(|| AppError::db(sqlx::Error::RowNotFound))?;
        let raw = random_token("sb_src_");
        let result =
            sqlx::query("UPDATE sources SET token_hash=? WHERE id=? AND token_hash IS NULL")
                .bind(hash_secret(&raw))
                .bind(source)
                .execute(&state.pool)
                .await
                .map_err(AppError::db)?;
        if result.rows_affected() == 1 {
            token = Some(raw);
        }
    }
    let state_value = match status.as_str() {
        "pending" => PairingState::Pending,
        "approved" => PairingState::Approved,
        "rejected" => PairingState::Rejected,
        _ => PairingState::Expired,
    };
    Ok(Json(PairingPollResponse {
        status: state_value,
        source_id: source_id.and_then(|s| Uuid::parse_str(&s).ok()),
        source_token: token,
    }))
}

async fn list_pairings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<PairingListResponse>, AppError> {
    owner_auth(&state, &headers, false).await?;
    sqlx::query("UPDATE pairing_requests SET status='expired',decided_at=? WHERE status='pending' AND expires_at<=?").bind(Utc::now()).bind(Utc::now()).execute(&state.pool).await.map_err(AppError::db)?;
    let rows = sqlx::query("SELECT id,code,display_name,created_at,expires_at FROM pairing_requests WHERE status='pending' ORDER BY created_at").fetch_all(&state.pool).await.map_err(AppError::db)?;
    Ok(Json(PairingListResponse {
        pairings: rows
            .into_iter()
            .filter_map(|r| {
                Some(PairingView {
                    id: Uuid::parse_str(r.get("id")).ok()?,
                    code: r.get("code"),
                    display_name: r.get("display_name"),
                    created_at: r.get("created_at"),
                    expires_at: r.get("expires_at"),
                })
            })
            .collect(),
    }))
}

async fn approve_pairing(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<SourceView>, AppError> {
    owner_auth(&state, &headers, true).await?;
    let name: String = sqlx::query_scalar("SELECT display_name FROM pairing_requests WHERE id=?")
        .bind(id.to_string())
        .fetch_optional(&state.pool)
        .await
        .map_err(AppError::db)?
        .ok_or_else(AppError::not_found)?;
    let source_id = Uuid::new_v4();
    let now = Utc::now();
    let mut tx = state.pool.begin().await.map_err(AppError::db)?;
    sqlx::query("INSERT INTO sources(id,display_name,created_at) VALUES(?,?,?)")
        .bind(source_id.to_string())
        .bind(&name)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(AppError::db)?;
    let result = sqlx::query("UPDATE pairing_requests SET status='approved',decided_at=?,source_id=? WHERE id=? AND status='pending' AND expires_at>?").bind(now).bind(source_id.to_string()).bind(id.to_string()).bind(now).execute(&mut *tx).await.map_err(AppError::db)?;
    if result.rows_affected() != 1 {
        tx.rollback().await.map_err(AppError::db)?;
        sqlx::query("UPDATE pairing_requests SET status='expired',decided_at=? WHERE id=? AND status='pending' AND expires_at<=?").bind(now).bind(id.to_string()).bind(now).execute(&state.pool).await.map_err(AppError::db)?;
        return Err(AppError::conflict("pairing was already decided"));
    }
    tx.commit().await.map_err(AppError::db)?;
    Ok(Json(SourceView {
        id: source_id,
        display_name: name,
        created_at: now,
        last_seen_at: None,
        revoked_at: None,
    }))
}

async fn reject_pairing(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    owner_auth(&state, &headers, true).await?;
    let result = sqlx::query("UPDATE pairing_requests SET status=CASE WHEN expires_at<=? THEN 'expired' ELSE 'rejected' END,decided_at=? WHERE id=? AND status='pending'").bind(Utc::now()).bind(Utc::now()).bind(id.to_string()).execute(&state.pool).await.map_err(AppError::db)?;
    if result.rows_affected() == 0 {
        return Err(AppError::conflict("pairing is no longer pending"));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn source_auth(state: &AppState, headers: &HeaderMap) -> Result<SourceView, AppError> {
    let authorization = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(AppError::unauthorized)?;
    let rows = sqlx::query("SELECT id,display_name,token_hash,created_at,last_seen_at,revoked_at FROM sources WHERE token_hash IS NOT NULL AND revoked_at IS NULL").fetch_all(&state.pool).await.map_err(AppError::db)?;
    for row in rows {
        let hash: String = row.get("token_hash");
        if verify_secret(authorization, &hash) {
            return source_from_row(&row);
        }
    }
    Err(AppError::unauthorized())
}

fn source_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<SourceView, AppError> {
    Ok(SourceView {
        id: Uuid::parse_str(row.get("id")).map_err(|_| AppError::db(sqlx::Error::RowNotFound))?,
        display_name: row.get("display_name"),
        created_at: row.get("created_at"),
        last_seen_at: row.get("last_seen_at"),
        revoked_at: row.get("revoked_at"),
    })
}

async fn source_self(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SourceView>, AppError> {
    Ok(Json(source_auth(&state, &headers).await?))
}

async fn list_sources(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SourceListResponse>, AppError> {
    owner_auth(&state, &headers, false).await?;
    let rows = sqlx::query("SELECT id,display_name,created_at,last_seen_at,revoked_at FROM sources ORDER BY created_at DESC").fetch_all(&state.pool).await.map_err(AppError::db)?;
    Ok(Json(SourceListResponse {
        sources: rows.iter().map(source_from_row).collect::<Result<_, _>>()?,
    }))
}

async fn rename_source(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(request): Json<RenameRequest>,
) -> Result<Json<SourceView>, AppError> {
    owner_auth(&state, &headers, true).await?;
    let name = validate_name(&request.name).map_err(AppError::bad)?;
    let result = sqlx::query("UPDATE sources SET display_name=? WHERE id=?")
        .bind(name)
        .bind(id.to_string())
        .execute(&state.pool)
        .await
        .map_err(AppError::db)?;
    if result.rows_affected() == 0 {
        return Err(AppError::not_found());
    }
    let row = sqlx::query(
        "SELECT id,display_name,created_at,last_seen_at,revoked_at FROM sources WHERE id=?",
    )
    .bind(id.to_string())
    .fetch_one(&state.pool)
    .await
    .map_err(AppError::db)?;
    Ok(Json(source_from_row(&row)?))
}

async fn revoke_source(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    owner_auth(&state, &headers, true).await?;
    let result = sqlx::query(
        "UPDATE sources SET revoked_at=?,token_hash=NULL WHERE id=? AND revoked_at IS NULL",
    )
    .bind(Utc::now())
    .bind(id.to_string())
    .execute(&state.pool)
    .await
    .map_err(AppError::db)?;
    if result.rows_affected() == 0 {
        return Err(AppError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn create_receiver(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ReceiverCreateRequest>,
) -> Result<(StatusCode, Json<ReceiverView>), AppError> {
    owner_auth(&state, &headers, true).await?;
    let name = validate_name(&request.name).map_err(AppError::bad)?;
    let tags = validate_tags(&request.tags).map_err(AppError::bad)?;
    validate_subscription(&request.subscription).map_err(AppError::bad)?;
    let id = Uuid::new_v4();
    let now = Utc::now();
    let tags_json = serde_json::to_string(&tags).map_err(|_| AppError::bad("invalid tags"))?;
    let result = sqlx::query("INSERT INTO receivers(id,name,tags_json,endpoint,p256dh,auth,enabled,created_at,updated_at) VALUES(?,?,?,?,?,?,1,?,?) ON CONFLICT(endpoint) DO UPDATE SET name=excluded.name,tags_json=excluded.tags_json,p256dh=excluded.p256dh,auth=excluded.auth,enabled=1,updated_at=excluded.updated_at,revoked_at=NULL")
        .bind(id.to_string()).bind(&name).bind(tags_json).bind(&request.subscription.endpoint).bind(&request.subscription.p256dh).bind(&request.subscription.auth).bind(now).bind(now).execute(&state.pool).await.map_err(AppError::db)?;
    let actual_id: String = sqlx::query_scalar("SELECT id FROM receivers WHERE endpoint=?")
        .bind(&request.subscription.endpoint)
        .fetch_one(&state.pool)
        .await
        .map_err(AppError::db)?;
    let view = ReceiverView {
        id: Uuid::parse_str(&actual_id).map_err(|_| AppError::db(sqlx::Error::RowNotFound))?,
        name,
        tags,
        enabled: true,
        created_at: now,
    };
    let status = if result.rows_affected() > 0 {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(view)))
}

async fn list_receivers(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<ReceiverListResponse>, AppError> {
    owner_auth(&state, &headers, false).await?;
    let rows = sqlx::query("SELECT id,name,tags_json,enabled,created_at FROM receivers WHERE revoked_at IS NULL ORDER BY created_at DESC").fetch_all(&state.pool).await.map_err(AppError::db)?;
    Ok(Json(ReceiverListResponse {
        receivers: rows
            .into_iter()
            .filter_map(|r| {
                Some(ReceiverView {
                    id: Uuid::parse_str(r.get("id")).ok()?,
                    name: r.get("name"),
                    tags: serde_json::from_str(r.get("tags_json")).ok()?,
                    enabled: r.get::<i64, _>("enabled") != 0,
                    created_at: r.get("created_at"),
                })
            })
            .collect(),
    }))
}

async fn update_receiver(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(request): Json<ReceiverUpdateRequest>,
) -> Result<Json<ReceiverView>, AppError> {
    owner_auth(&state, &headers, true).await?;
    let row = sqlx::query(
        "SELECT name,tags_json,enabled,created_at FROM receivers WHERE id=? AND revoked_at IS NULL",
    )
    .bind(id.to_string())
    .fetch_optional(&state.pool)
    .await
    .map_err(AppError::db)?
    .ok_or_else(AppError::not_found)?;
    let name = request
        .name
        .as_deref()
        .map(validate_name)
        .transpose()
        .map_err(AppError::bad)?
        .unwrap_or_else(|| row.get("name"));
    let tags = request
        .tags
        .as_ref()
        .map(|v| validate_tags(v))
        .transpose()
        .map_err(AppError::bad)?
        .unwrap_or_else(|| serde_json::from_str(row.get("tags_json")).unwrap_or_default());
    let enabled = request
        .enabled
        .unwrap_or_else(|| row.get::<i64, _>("enabled") != 0);
    sqlx::query("UPDATE receivers SET name=?,tags_json=?,enabled=?,updated_at=? WHERE id=?")
        .bind(&name)
        .bind(serde_json::to_string(&tags).unwrap())
        .bind(enabled)
        .bind(Utc::now())
        .bind(id.to_string())
        .execute(&state.pool)
        .await
        .map_err(AppError::db)?;
    Ok(Json(ReceiverView {
        id,
        name,
        tags,
        enabled,
        created_at: row.get("created_at"),
    }))
}

async fn revoke_receiver(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    owner_auth(&state, &headers, true).await?;
    let result = sqlx::query("UPDATE receivers SET revoked_at=?,enabled=0,updated_at=? WHERE id=? AND revoked_at IS NULL").bind(Utc::now()).bind(Utc::now()).bind(id.to_string()).execute(&state.pool).await.map_err(AppError::db)?;
    if result.rows_affected() == 0 {
        return Err(AppError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
struct PushPayload<'a> {
    title: &'a str,
    body: &'a str,
    url: &'a str,
    event_id: Uuid,
}

async fn submit_ring(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<RingRequest>,
) -> Result<(StatusCode, Json<RingAcceptedResponse>), AppError> {
    let source = source_auth(&state, &headers).await?;
    if !state
        .limiter
        .check(format!("ring:{}", source.id), 120, Duration::from_secs(60))
    {
        return Err(AppError::rate_limited());
    }
    let message = validate_message(request.message.as_deref()).map_err(AppError::bad)?;
    let tags = validate_tags(&request.target_tags).map_err(AppError::bad)?;
    let ring_guard = state.ring_lock.lock().await;
    let mut tx = state.pool.begin().await.map_err(AppError::db)?;
    if let Some(row) = sqlx::query(
        "SELECT created_at,matched_receivers FROM rings WHERE source_id=? AND event_id=?",
    )
    .bind(source.id.to_string())
    .bind(request.event_id.to_string())
    .fetch_optional(&mut *tx)
    .await
    .map_err(AppError::db)?
    {
        let response = RingAcceptedResponse {
            event_id: request.event_id,
            accepted_at: row.get("created_at"),
            duplicate: true,
            matched_receivers: row.get::<i64, _>("matched_receivers") as u32,
        };
        tx.commit().await.map_err(AppError::db)?;
        drop(ring_guard);
        return Ok((StatusCode::OK, Json(response)));
    }
    let now = Utc::now();
    let receiver_rows = sqlx::query("SELECT id,endpoint,p256dh,auth,tags_json FROM receivers WHERE enabled=1 AND revoked_at IS NULL").fetch_all(&mut *tx).await.map_err(AppError::db)?;
    let mut receivers = Vec::new();
    for row in receiver_rows {
        let receiver_tags: Vec<String> =
            serde_json::from_str(row.get("tags_json")).unwrap_or_default();
        if tags.is_empty() || tags.iter().any(|tag| receiver_tags.contains(tag)) {
            receivers.push((
                row.get::<String, _>("id"),
                PushSubscription {
                    endpoint: row.get("endpoint"),
                    p256dh: row.get("p256dh"),
                    auth: row.get("auth"),
                },
            ));
        }
    }
    let result = sqlx::query("INSERT INTO rings(event_id,source_id,source_name,message,target_tags_json,created_at,matched_receivers) VALUES(?,?,?,?,?,?,?)")
        .bind(request.event_id.to_string()).bind(source.id.to_string()).bind(&source.display_name).bind(&message).bind(serde_json::to_string(&tags).unwrap()).bind(now).bind(receivers.len() as i64).execute(&mut *tx).await.map_err(AppError::db)?;
    let ring_id = result.last_insert_rowid();
    for (receiver_id, _) in &receivers {
        sqlx::query("INSERT INTO deliveries(ring_id,receiver_id,status) VALUES(?,?,'queued')")
            .bind(ring_id)
            .bind(receiver_id)
            .execute(&mut *tx)
            .await
            .map_err(AppError::db)?;
    }
    sqlx::query("UPDATE sources SET last_seen_at=? WHERE id=?")
        .bind(now)
        .bind(source.id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(AppError::db)?;
    let retention: i64 = sqlx::query_scalar(
        "SELECT CAST(value AS INTEGER) FROM settings WHERE key='history_retention_days'",
    )
    .fetch_one(&mut *tx)
    .await
    .unwrap_or(14);
    sqlx::query("DELETE FROM rings WHERE created_at < ?")
        .bind(now - ChronoDuration::days(retention))
        .execute(&mut *tx)
        .await
        .map_err(AppError::db)?;
    tx.commit().await.map_err(AppError::db)?;
    drop(ring_guard);
    let payload = serde_json::to_vec(&PushPayload {
        title: "Shellbell",
        body: message.as_deref().unwrap_or("Your terminal is ready"),
        url: "/?view=rings",
        event_id: request.event_id,
    })
    .map_err(|_| AppError::bad("could not serialize notification"))?;
    for (receiver_id, subscription) in receivers.iter() {
        let outcome = state.push.deliver(subscription, &payload).await;
        let (status, diagnostic, permanent) = match outcome {
            DeliveryOutcome::Delivered => ("delivered", None, false),
            DeliveryOutcome::PermanentFailure(value) => ("permanent_failure", Some(value), true),
            DeliveryOutcome::TransientFailure(value) => ("transient_failure", Some(value), false),
        };
        sqlx::query("UPDATE deliveries SET attempted_at=?,status=?,diagnostic=? WHERE ring_id=? AND receiver_id=?").bind(Utc::now()).bind(status).bind(diagnostic).bind(ring_id).bind(receiver_id).execute(&state.pool).await.map_err(AppError::db)?;
        if permanent {
            sqlx::query("UPDATE receivers SET enabled=0,updated_at=? WHERE id=?")
                .bind(Utc::now())
                .bind(receiver_id)
                .execute(&state.pool)
                .await
                .map_err(AppError::db)?;
        }
    }
    Ok((
        StatusCode::ACCEPTED,
        Json(RingAcceptedResponse {
            event_id: request.event_id,
            accepted_at: now,
            duplicate: false,
            matched_receivers: receivers.len() as u32,
        }),
    ))
}

#[derive(Deserialize)]
struct RingQuery {
    limit: Option<u32>,
}
async fn list_rings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<RingQuery>,
) -> Result<Json<RingListResponse>, AppError> {
    owner_auth(&state, &headers, false).await?;
    let limit = query.limit.unwrap_or(100).clamp(1, 200);
    let rows = sqlx::query("SELECT event_id,source_id,source_name,message,target_tags_json,created_at FROM rings ORDER BY created_at DESC LIMIT ?").bind(limit).fetch_all(&state.pool).await.map_err(AppError::db)?;
    Ok(Json(RingListResponse {
        rings: rows
            .into_iter()
            .filter_map(|r| {
                Some(RingView {
                    event_id: Uuid::parse_str(r.get("event_id")).ok()?,
                    source_id: Uuid::parse_str(r.get("source_id")).ok()?,
                    source_name: r.get("source_name"),
                    message: r.get("message"),
                    target_tags: serde_json::from_str(r.get("target_tags_json")).ok()?,
                    created_at: r.get("created_at"),
                })
            })
            .collect(),
    }))
}

async fn get_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SettingsResponse>, AppError> {
    owner_auth(&state, &headers, false).await?;
    let days: String =
        sqlx::query_scalar("SELECT value FROM settings WHERE key='history_retention_days'")
            .fetch_one(&state.pool)
            .await
            .map_err(AppError::db)?;
    Ok(Json(SettingsResponse {
        history_retention_days: days.parse().unwrap_or(14),
        vapid_public_key: state.config.vapid_public_key,
    }))
}

async fn update_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<SettingsUpdateRequest>,
) -> Result<Json<SettingsResponse>, AppError> {
    owner_auth(&state, &headers, true).await?;
    if !(1..=90).contains(&request.history_retention_days) {
        return Err(AppError::bad(
            "history retention must be between 1 and 90 days",
        ));
    }
    sqlx::query("UPDATE settings SET value=? WHERE key='history_retention_days'")
        .bind(request.history_retention_days.to_string())
        .execute(&state.pool)
        .await
        .map_err(AppError::db)?;
    Ok(Json(SettingsResponse {
        history_retention_days: request.history_retention_days,
        vapid_public_key: state.config.vapid_public_key,
    }))
}

pub fn json_request(method: &str, uri: &str, body: impl Serialize) -> axum::http::Request<Body> {
    axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&body).expect("serialize request"),
        ))
        .expect("build request")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::push::FakePushDelivery;
    use http_body_util::BodyExt;
    use serde::de::DeserializeOwned;
    use std::sync::Arc;
    use tempfile::TempDir;
    use tower::ServiceExt;

    struct Harness {
        _dir: TempDir,
        app: Router,
        pool: SqlitePool,
        fake: FakePushDelivery,
        cookie: String,
        csrf: String,
    }

    impl Harness {
        async fn new(pairing_ttl: ChronoDuration) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let url = format!("sqlite://{}", dir.path().join("test.db").display());
            let pool = open_database(&url).await.unwrap();
            let fake = FakePushDelivery::default();
            let mut config =
                RelayConfig::development("test-bootstrap-token-with-at-least-32-bytes");
            config.pairing_ttl = pairing_ttl;
            let app = build_app(pool.clone(), config, Arc::new(fake.clone()))
                .await
                .unwrap();
            let response = app
                .clone()
                .oneshot(json_request(
                    "POST",
                    "/api/owner/bootstrap",
                    BootstrapRequest {
                        bootstrap_token: "test-bootstrap-token-with-at-least-32-bytes".into(),
                    },
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let cookies: Vec<String> = response
                .headers()
                .get_all(header::SET_COOKIE)
                .iter()
                .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_owned())
                .collect();
            let csrf = cookies
                .iter()
                .find_map(|v| v.strip_prefix("sb_csrf=").map(str::to_owned))
                .unwrap();
            Self {
                _dir: dir,
                app,
                pool,
                fake,
                cookie: cookies.join("; "),
                csrf,
            }
        }

        fn owner(
            &self,
            mut request: axum::http::Request<Body>,
            mutation: bool,
        ) -> axum::http::Request<Body> {
            request
                .headers_mut()
                .insert(header::COOKIE, HeaderValue::from_str(&self.cookie).unwrap());
            if mutation {
                request
                    .headers_mut()
                    .insert("x-csrf-token", HeaderValue::from_str(&self.csrf).unwrap());
            }
            request
        }

        async fn pair(&self, name: &str) -> PairingCreateResponse {
            let response = self
                .app
                .clone()
                .oneshot(json_request(
                    "POST",
                    "/api/pairings",
                    PairingCreateRequest {
                        display_name: name.into(),
                    },
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::CREATED);
            body(response).await
        }

        async fn approve_and_poll(&self, pair: &PairingCreateResponse) -> PairingPollResponse {
            let response = self
                .app
                .clone()
                .oneshot(self.owner(
                    json_request(
                        "POST",
                        &format!("/api/pairings/{}/approve", pair.id),
                        serde_json::json!({}),
                    ),
                    true,
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let response = self
                .app
                .clone()
                .oneshot(
                    axum::http::Request::get(format!("/api/pairings/{}", pair.id))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            body(response).await
        }
    }

    async fn body<T: DeserializeOwned>(response: Response) -> T {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn bearer(mut request: axum::http::Request<Body>, token: &str) -> axum::http::Request<Body> {
        request.headers_mut().insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
        );
        request
    }

    #[tokio::test]
    async fn migration_and_owner_authorization_work() {
        let harness = Harness::new(ChronoDuration::minutes(10)).await;
        let version: i64 = sqlx::query_scalar(
            "SELECT version FROM _sqlx_migrations ORDER BY version DESC LIMIT 1",
        )
        .fetch_one(&harness.pool)
        .await
        .unwrap();
        assert_eq!(version, 1);
        let response = harness
            .app
            .clone()
            .oneshot(
                axum::http::Request::get("/api/sources")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let response = harness
            .app
            .clone()
            .oneshot(
                harness.owner(
                    axum::http::Request::get("/api/sources")
                        .body(Body::empty())
                        .unwrap(),
                    false,
                ),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let missing_csrf = harness
            .app
            .clone()
            .oneshot(harness.owner(
                json_request(
                    "PUT",
                    "/api/settings",
                    SettingsUpdateRequest {
                        history_retention_days: 7,
                    },
                ),
                false,
            ))
            .await
            .unwrap();
        assert_eq!(missing_csrf.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn pairing_approval_is_single_use_and_token_is_returned_once() {
        let harness = Harness::new(ChronoDuration::minutes(10)).await;
        let pair = harness.pair("workstation").await;
        assert_eq!(pair.code.len(), PAIRING_CODE_LEN);
        let poll = harness.approve_and_poll(&pair).await;
        assert_eq!(poll.status, PairingState::Approved);
        assert!(
            poll.source_token
                .as_deref()
                .is_some_and(|v| v.starts_with("sb_src_"))
        );
        let second: PairingPollResponse = body(
            harness
                .app
                .clone()
                .oneshot(
                    axum::http::Request::get(format!("/api/pairings/{}", pair.id))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(second.status, PairingState::Approved);
        assert!(second.source_token.is_none());
        let response = harness
            .app
            .clone()
            .oneshot(harness.owner(
                json_request(
                    "POST",
                    &format!("/api/pairings/{}/approve", pair.id),
                    serde_json::json!({}),
                ),
                true,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn concurrent_pairing_approval_and_poll_have_single_winners() {
        let harness = Harness::new(ChronoDuration::minutes(10)).await;
        let pair = harness.pair("race-source").await;
        let path = format!("/api/pairings/{}/approve", pair.id);
        let first = harness.owner(json_request("POST", &path, serde_json::json!({})), true);
        let second = harness.owner(json_request("POST", &path, serde_json::json!({})), true);
        let (first, second) = tokio::join!(
            harness.app.clone().oneshot(first),
            harness.app.clone().oneshot(second)
        );
        let mut statuses = [first.unwrap().status(), second.unwrap().status()];
        statuses.sort();
        assert_eq!(statuses, [StatusCode::OK, StatusCode::CONFLICT]);

        let poll_path = format!("/api/pairings/{}", pair.id);
        let first = axum::http::Request::get(&poll_path)
            .body(Body::empty())
            .unwrap();
        let second = axum::http::Request::get(&poll_path)
            .body(Body::empty())
            .unwrap();
        let (first, second) = tokio::join!(
            harness.app.clone().oneshot(first),
            harness.app.clone().oneshot(second)
        );
        let first: PairingPollResponse = body(first.unwrap()).await;
        let second: PairingPollResponse = body(second.unwrap()).await;
        assert_eq!(
            usize::from(first.source_token.is_some()) + usize::from(second.source_token.is_some()),
            1
        );
    }

    #[tokio::test]
    async fn rejected_and_expired_pairings_cannot_be_approved() {
        let harness = Harness::new(ChronoDuration::minutes(10)).await;
        let pair = harness.pair("rejected").await;
        let response = harness
            .app
            .clone()
            .oneshot(harness.owner(
                json_request(
                    "POST",
                    &format!("/api/pairings/{}/reject", pair.id),
                    serde_json::json!({}),
                ),
                true,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        let response = harness
            .app
            .clone()
            .oneshot(harness.owner(
                json_request(
                    "POST",
                    &format!("/api/pairings/{}/approve", pair.id),
                    serde_json::json!({}),
                ),
                true,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);

        let expired = Harness::new(ChronoDuration::seconds(-1)).await;
        let pair = expired.pair("expired").await;
        let poll: PairingPollResponse = body(
            expired
                .app
                .clone()
                .oneshot(
                    axum::http::Request::get(format!("/api/pairings/{}", pair.id))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(poll.status, PairingState::Expired);
        let response = expired
            .app
            .clone()
            .oneshot(expired.owner(
                json_request(
                    "POST",
                    &format!("/api/pairings/{}/approve", pair.id),
                    serde_json::json!({}),
                ),
                true,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn source_is_send_only_and_revocation_is_immediate() {
        let harness = Harness::new(ChronoDuration::minutes(10)).await;
        let pair = harness.pair("source").await;
        let poll = harness.approve_and_poll(&pair).await;
        let token = poll.source_token.unwrap();
        let self_response = harness
            .app
            .clone()
            .oneshot(bearer(
                axum::http::Request::get("/api/sources/self")
                    .body(Body::empty())
                    .unwrap(),
                &token,
            ))
            .await
            .unwrap();
        assert_eq!(self_response.status(), StatusCode::OK);
        let forbidden = harness
            .app
            .clone()
            .oneshot(bearer(
                axum::http::Request::get("/api/rings")
                    .body(Body::empty())
                    .unwrap(),
                &token,
            ))
            .await
            .unwrap();
        assert_eq!(forbidden.status(), StatusCode::UNAUTHORIZED);
        let source_id = poll.source_id.unwrap();
        let revoked = harness
            .app
            .clone()
            .oneshot(
                harness.owner(
                    axum::http::Request::delete(format!("/api/sources/{source_id}"))
                        .body(Body::empty())
                        .unwrap(),
                    true,
                ),
            )
            .await
            .unwrap();
        assert_eq!(revoked.status(), StatusCode::NO_CONTENT);
        let self_response = harness
            .app
            .clone()
            .oneshot(bearer(
                axum::http::Request::get("/api/sources/self")
                    .body(Body::empty())
                    .unwrap(),
                &token,
            ))
            .await
            .unwrap();
        assert_eq!(self_response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn receiver_routing_disabled_exclusion_and_ring_idempotency() {
        let harness = Harness::new(ChronoDuration::minutes(10)).await;
        let pair = harness.pair("source").await;
        let poll = harness.approve_and_poll(&pair).await;
        let token = poll.source_token.unwrap();
        for (name, tag) in [("phone", "phone"), ("pc", "pc")] {
            let request = ReceiverCreateRequest {
                name: name.into(),
                tags: vec![tag.into()],
                subscription: PushSubscriptionInput {
                    endpoint: format!("https://push.example/{name}"),
                    p256dh: "valid-p256dh".into(),
                    auth: "valid-auth".into(),
                },
            };
            let response = harness
                .app
                .clone()
                .oneshot(harness.owner(json_request("POST", "/api/receivers", request), true))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::CREATED);
        }
        let list: ReceiverListResponse = body(
            harness
                .app
                .clone()
                .oneshot(
                    harness.owner(
                        axum::http::Request::get("/api/receivers")
                            .body(Body::empty())
                            .unwrap(),
                        false,
                    ),
                )
                .await
                .unwrap(),
        )
        .await;
        let pc = list.receivers.iter().find(|r| r.name == "pc").unwrap();
        let response = harness
            .app
            .clone()
            .oneshot(harness.owner(
                json_request(
                    "PATCH",
                    &format!("/api/receivers/{}", pc.id),
                    ReceiverUpdateRequest {
                        name: None,
                        tags: None,
                        enabled: Some(false),
                    },
                ),
                true,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let event_id = Uuid::new_v4();
        let ring = RingRequest {
            event_id,
            message: Some("ready".into()),
            target_tags: vec!["phone".into()],
        };
        let response = harness
            .app
            .clone()
            .oneshot(bearer(json_request("POST", "/api/rings", &ring), &token))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let accepted: RingAcceptedResponse = body(response).await;
        assert_eq!(accepted.matched_receivers, 1);
        assert!(!accepted.duplicate);
        assert_eq!(harness.fake.attempts.lock().await.len(), 1);
        let response = harness
            .app
            .clone()
            .oneshot(bearer(json_request("POST", "/api/rings", &ring), &token))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let accepted: RingAcceptedResponse = body(response).await;
        assert!(accepted.duplicate);
        assert_eq!(accepted.matched_receivers, 1);
        assert_eq!(harness.fake.attempts.lock().await.len(), 1);
        let rings: RingListResponse = body(
            harness
                .app
                .clone()
                .oneshot(
                    harness.owner(
                        axum::http::Request::get("/api/rings")
                            .body(Body::empty())
                            .unwrap(),
                        false,
                    ),
                )
                .await
                .unwrap(),
        )
        .await;
        assert_eq!(rings.rings.len(), 1);
        assert_eq!(rings.rings[0].message.as_deref(), Some("ready"));
    }

    #[tokio::test]
    async fn permanent_push_failure_disables_receiver_and_validation_errors_are_structured() {
        let harness = Harness::new(ChronoDuration::minutes(10)).await;
        *harness.fake.outcome.lock().await = Some(DeliveryOutcome::PermanentFailure("gone".into()));
        let pair = harness.pair("source").await;
        let poll = harness.approve_and_poll(&pair).await;
        let token = poll.source_token.unwrap();
        let receiver = ReceiverCreateRequest {
            name: "browser".into(),
            tags: vec!["pc".into()],
            subscription: PushSubscriptionInput {
                endpoint: "https://push.example/gone".into(),
                p256dh: "key".into(),
                auth: "auth".into(),
            },
        };
        let response = harness
            .app
            .clone()
            .oneshot(harness.owner(json_request("POST", "/api/receivers", receiver), true))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        let ring = RingRequest {
            event_id: Uuid::new_v4(),
            message: Some("x".repeat(MESSAGE_MAX + 1)),
            target_tags: vec![],
        };
        let response = harness
            .app
            .clone()
            .oneshot(bearer(json_request("POST", "/api/rings", ring), &token))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let error: ApiError = body(response).await;
        assert_eq!(error.error.code, "validation_error");
        let ring = RingRequest {
            event_id: Uuid::new_v4(),
            message: None,
            target_tags: vec![],
        };
        let response = harness
            .app
            .clone()
            .oneshot(bearer(json_request("POST", "/api/rings", ring), &token))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let enabled: i64 = sqlx::query_scalar(
            "SELECT enabled FROM receivers WHERE endpoint='https://push.example/gone'",
        )
        .fetch_one(&harness.pool)
        .await
        .unwrap();
        assert_eq!(enabled, 0);
        let status: String = sqlx::query_scalar("SELECT status FROM deliveries LIMIT 1")
            .fetch_one(&harness.pool)
            .await
            .unwrap();
        assert_eq!(status, "permanent_failure");
    }

    #[tokio::test]
    async fn pairing_rate_limit_is_enforced() {
        let harness = Harness::new(ChronoDuration::minutes(10)).await;
        for index in 0..12 {
            let mut request = json_request(
                "POST",
                "/api/pairings",
                PairingCreateRequest {
                    display_name: format!("source-{index}"),
                },
            );
            request
                .headers_mut()
                .insert("x-forwarded-for", HeaderValue::from_static("203.0.113.1"));
            let response = harness.app.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::CREATED);
        }
        let mut request = json_request(
            "POST",
            "/api/pairings",
            PairingCreateRequest {
                display_name: "blocked".into(),
            },
        );
        request
            .headers_mut()
            .insert("x-forwarded-for", HeaderValue::from_static("203.0.113.1"));
        let response = harness.app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }
}
