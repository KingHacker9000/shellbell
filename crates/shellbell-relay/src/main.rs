use anyhow::{Context, bail};
use shellbell_core::{hash_secret, random_token};
use shellbell_relay::{RelayConfig, build_app, open_database, push::VapidPushDelivery};
use std::{
    env, fs,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

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
    let static_dir = env::var_os("SHELLBELL_STATIC_DIR")
        .map(PathBuf::from)
        .or_else(|| Some(PathBuf::from("/app/pwa")));
    let config = RelayConfig {
        bootstrap_token_hash: hash_secret(&bootstrap),
        secure_cookies,
        session_ttl: chrono::Duration::hours(24),
        pairing_ttl: chrono::Duration::minutes(10),
        default_retention_days: retention,
        vapid_public_key: vapid_public,
        static_dir,
    };
    let pool = open_database(&database_url).await?;
    let app = build_app(
        pool,
        config,
        Arc::new(VapidPushDelivery::new(
            vapid_private,
            vapid_subject,
            Duration::from_secs(8),
        )),
    )
    .await?;
    let address: SocketAddr = env::var("SHELLBELL_LISTEN")
        .unwrap_or_else(|_| "0.0.0.0:8080".into())
        .parse()?;
    tracing::info!(%address, bootstrap_file = %data_dir.join("owner-bootstrap-token").display(), "relay listening; bootstrap secret is never logged");
    axum::serve(TcpListener::bind(address).await?, app.into_make_service()).await?;
    Ok(())
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
