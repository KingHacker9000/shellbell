use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use directories::ProjectDirs;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use shellbell_protocol::{
    HealthResponse, PairingCreateRequest, PairingCreateResponse, PairingPollResponse, PairingState,
    RingAcceptedResponse, RingRequest, SourceView,
};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::time::{Instant, sleep};
use url::Url;
use uuid::Uuid;

#[derive(Parser)]
#[command(
    name = "shellbell",
    version,
    about = "Private terminal attention notifier"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Pair this source machine with a Shellbell relay.
    Pair { relay_url: Url },
    /// Send a manual notification without inspecting shell activity.
    Ring {
        message: Option<String>,
        #[arg(long, value_enum, default_value = "all")]
        to: Target,
    },
    /// Check relay connectivity and source credentials.
    Doctor,
}

#[derive(Clone, Debug, ValueEnum)]
enum Target {
    Phone,
    Pc,
    All,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct StoredConfig {
    relay_url: Url,
    source_id: Uuid,
    source_token: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .user_agent(concat!("shellbell/", env!("CARGO_PKG_VERSION")))
        .build()?;
    match Cli::parse().command {
        Command::Pair { relay_url } => pair(&client, relay_url, &config_path()?).await,
        Command::Ring { message, to } => ring(&client, message, to, &config_path()?).await,
        Command::Doctor => doctor(&client, &config_path()?).await,
    }
}

fn config_path() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("com", "shellbell", "shellbell")
        .context("could not determine the per-user configuration directory")?;
    Ok(dirs.config_dir().join("config.json"))
}

fn validate_relay_url(mut url: Url) -> Result<Url> {
    let localhost = matches!(
        url.host_str(),
        Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
    );
    if url.scheme() != "https" && !(url.scheme() == "http" && localhost) {
        bail!("relay URL must use HTTPS (HTTP is allowed only for localhost)");
    }
    if url.cannot_be_a_base() || url.host_str().is_none() {
        bail!("relay URL must be an absolute HTTP URL");
    }
    url.set_query(None);
    url.set_fragment(None);
    if !url.path().ends_with('/') {
        url.set_path(&format!("{}/", url.path()));
    }
    Ok(url)
}

fn endpoint(base: &Url, path: &str) -> Result<Url> {
    base.join(path)
        .context("could not construct relay endpoint")
}

async fn pair(client: &Client, relay_url: Url, path: &Path) -> Result<()> {
    let relay_url = validate_relay_url(relay_url)?;
    let display_name = hostname::get()
        .context("could not determine machine name")?
        .to_string_lossy()
        .trim()
        .to_owned();
    let display_name = if display_name.is_empty() {
        "Shellbell source".into()
    } else {
        display_name
    };
    let response: PairingCreateResponse = send_json(
        client
            .post(endpoint(&relay_url, "api/pairings")?)
            .json(&PairingCreateRequest { display_name }),
    )
    .await
    .context("could not create pairing request")?;
    println!("Pairing code: {}", response.code);
    println!(
        "Approve this code in the Shellbell PWA before {}.",
        response.expires_at.to_rfc3339()
    );
    let deadline = Instant::now()
        + response
            .expires_at
            .signed_duration_since(chrono::Utc::now())
            .to_std()
            .unwrap_or_default();
    loop {
        if Instant::now() >= deadline {
            bail!("pairing request expired before it was approved");
        }
        sleep(Duration::from_secs(2)).await;
        let poll_url = endpoint(&relay_url, &format!("api/pairings/{}", response.id))?;
        let poll: PairingPollResponse = get_json_retry(client, poll_url, None)
            .await
            .context("could not poll pairing request")?;
        match poll.status {
            PairingState::Pending => continue,
            PairingState::Rejected => bail!("pairing request was rejected"),
            PairingState::Expired => bail!("pairing request expired"),
            PairingState::Approved => {
                let source_id = poll
                    .source_id
                    .context("approved pairing did not include a source identity")?;
                let source_token = poll
                    .source_token
                    .context("pairing token was already retrieved; create a new pairing request")?;
                save_config(
                    path,
                    &StoredConfig {
                        relay_url,
                        source_id,
                        source_token,
                    },
                )?;
                println!(
                    "Paired successfully as source {source_id}. Credentials were stored securely."
                );
                return Ok(());
            }
        }
    }
}

async fn ring(client: &Client, message: Option<String>, target: Target, path: &Path) -> Result<()> {
    let config = load_config(path)?;
    let target_tags = match target {
        Target::Phone => vec!["phone".into()],
        Target::Pc => vec!["pc".into()],
        Target::All => vec![],
    };
    let event_id = Uuid::new_v4();
    let response: RingAcceptedResponse = send_json(
        client
            .post(endpoint(&config.relay_url, "api/rings")?)
            .bearer_auth(&config.source_token)
            .json(&RingRequest {
                event_id,
                message,
                target_tags,
            }),
    )
    .await
    .context("relay rejected the ring")?;
    println!(
        "Ring {} ({} receiver{} matched).",
        if response.duplicate {
            "already accepted"
        } else {
            "accepted"
        },
        response.matched_receivers,
        if response.matched_receivers == 1 {
            ""
        } else {
            "s"
        }
    );
    Ok(())
}

async fn doctor(client: &Client, path: &Path) -> Result<()> {
    let config = load_config(path)?;
    let _: HealthResponse = get_json_retry(client, endpoint(&config.relay_url, "health")?, None)
        .await
        .context("relay health check failed")?;
    let source: SourceView = get_json_retry(
        client,
        endpoint(&config.relay_url, "api/sources/self")?,
        Some(&config.source_token),
    )
    .await
    .context("source credential validation failed")?;
    println!(
        "Relay: healthy\nSource: {} ({})\nCredentials: valid",
        source.display_name, source.id
    );
    Ok(())
}

async fn send_json<T: DeserializeOwned>(request: reqwest::RequestBuilder) -> Result<T> {
    let response = request.send().await?;
    if !response.status().is_success() {
        return Err(response_error(response).await);
    }
    Ok(response.json().await?)
}

async fn get_json_retry<T: DeserializeOwned>(
    client: &Client,
    url: Url,
    bearer: Option<&str>,
) -> Result<T> {
    let mut last_error = None;
    for attempt in 0..3 {
        let mut request = client.get(url.clone());
        if let Some(token) = bearer {
            request = request.bearer_auth(token);
        }
        match request.send().await {
            Ok(response) if response.status().is_success() => return Ok(response.json().await?),
            Ok(response) if response.status().is_server_error() && attempt < 2 => {
                last_error = Some(response_error(response).await)
            }
            Ok(response) => return Err(response_error(response).await),
            Err(error) if (error.is_timeout() || error.is_connect()) && attempt < 2 => {
                last_error = Some(error.into())
            }
            Err(error) => return Err(error.into()),
        }
        sleep(Duration::from_millis(250 * (attempt + 1))).await;
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("request failed")))
}

async fn response_error(response: reqwest::Response) -> anyhow::Error {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if let Ok(error) = serde_json::from_str::<shellbell_protocol::ApiError>(&body) {
        anyhow::anyhow!("{}: {}", error.error.code, error.error.message)
    } else if status == StatusCode::UNAUTHORIZED {
        anyhow::anyhow!("unauthorized; run `shellbell pair <relay-url>` again")
    } else {
        anyhow::anyhow!("relay returned HTTP {status}")
    }
}

fn load_config(path: &Path) -> Result<StoredConfig> {
    let bytes = fs::read(path).with_context(|| format!("Shellbell is not paired; run `shellbell pair <relay-url>` (configuration expected at {})", path.display()))?;
    serde_json::from_slice(&bytes)
        .context("Shellbell configuration is invalid; pair this machine again")
}

fn save_config(path: &Path, config: &StoredConfig) -> Result<()> {
    let parent = path.parent().context("configuration path has no parent")?;
    fs::create_dir_all(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true).mode(0o600);
        let mut file = options.open(path)?;
        serde_json::to_writer_pretty(&mut file, config)?;
        file.sync_all()?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    fs::write(path, serde_json::to_vec_pretty(config)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_https_or_local_http_is_allowed() {
        assert!(validate_relay_url(Url::parse("https://relay.example").unwrap()).is_ok());
        assert!(validate_relay_url(Url::parse("http://localhost:8080").unwrap()).is_ok());
        assert!(validate_relay_url(Url::parse("http://relay.example").unwrap()).is_err());
    }
    #[test]
    fn stored_credentials_round_trip_without_displaying_token() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let config = StoredConfig {
            relay_url: Url::parse("https://relay.example/").unwrap(),
            source_id: Uuid::new_v4(),
            source_token: "sb_src_secret".into(),
        };
        save_config(&path, &config).unwrap();
        assert_eq!(load_config(&path).unwrap(), config);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}
