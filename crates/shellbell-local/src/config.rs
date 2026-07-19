use anyhow::{Context, Result, bail};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};
use url::Url;
use uuid::Uuid;

const DEFAULT_CONFIG: &str = r#"[activity]
minimum_active = "2m"
idle_for = "45s"

[delivery]
targets = ["phone"]

[display]
automatic_message = "Shell is ready"

[daemon]
queue_limit = 100
event_max_age = "24h"
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveConfig {
    pub minimum_active: Duration,
    pub idle_for: Duration,
    pub targets: Vec<String>,
    pub automatic_message: String,
    pub queue_limit: usize,
    pub event_max_age: Duration,
}

impl Default for EffectiveConfig {
    fn default() -> Self {
        Self {
            minimum_active: Duration::from_secs(120),
            idle_for: Duration::from_secs(45),
            targets: vec!["phone".into()],
            automatic_message: "Shell is ready".into(),
            queue_limit: 100,
            event_max_age: Duration::from_secs(24 * 60 * 60),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawConfig {
    activity: RawActivity,
    delivery: RawDelivery,
    display: RawDisplay,
    daemon: RawDaemon,
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawActivity {
    minimum_active: String,
    idle_for: String,
}

impl Default for RawActivity {
    fn default() -> Self {
        Self {
            minimum_active: "2m".into(),
            idle_for: "45s".into(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawDelivery {
    targets: Vec<String>,
}

impl Default for RawDelivery {
    fn default() -> Self {
        Self {
            targets: vec!["phone".into()],
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawDisplay {
    automatic_message: String,
}

impl Default for RawDisplay {
    fn default() -> Self {
        Self {
            automatic_message: "Shell is ready".into(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawDaemon {
    queue_limit: usize,
    event_max_age: String,
}

impl Default for RawDaemon {
    fn default() -> Self {
        Self {
            queue_limit: 100,
            event_max_age: "24h".into(),
        }
    }
}

impl EffectiveConfig {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let value = fs::read_to_string(path)
            .with_context(|| format!("could not read {}", path.display()))?;
        Self::parse(&value).with_context(|| format!("invalid configuration at {}", path.display()))
    }

    pub fn parse(value: &str) -> Result<Self> {
        let raw: RawConfig = toml::from_str(value).context("configuration is not valid TOML")?;
        let minimum_active = parse_bounded_duration(
            "activity.minimum_active",
            &raw.activity.minimum_active,
            Duration::from_millis(100),
            Duration::from_secs(7 * 24 * 60 * 60),
        )?;
        let idle_for = parse_bounded_duration(
            "activity.idle_for",
            &raw.activity.idle_for,
            Duration::from_millis(100),
            Duration::from_secs(24 * 60 * 60),
        )?;
        let event_max_age = parse_bounded_duration(
            "daemon.event_max_age",
            &raw.daemon.event_max_age,
            Duration::from_secs(60),
            Duration::from_secs(30 * 24 * 60 * 60),
        )?;
        if !(1..=10_000).contains(&raw.daemon.queue_limit) {
            bail!("daemon.queue_limit must be between 1 and 10000");
        }
        let targets = shellbell_protocol::validate_tags(&raw.delivery.targets)
            .map_err(|error| anyhow::anyhow!("delivery.targets: {error}"))?;
        let automatic_message =
            shellbell_protocol::validate_message(Some(&raw.display.automatic_message))
                .map_err(|error| anyhow::anyhow!("display.automatic_message: {error}"))?
                .unwrap_or_default();
        if automatic_message.is_empty() {
            bail!("display.automatic_message must not be empty");
        }
        Ok(Self {
            minimum_active,
            idle_for,
            targets,
            automatic_message,
            queue_limit: raw.daemon.queue_limit,
            event_max_age,
        })
    }
}

fn parse_bounded_duration(
    name: &str,
    value: &str,
    minimum: Duration,
    maximum: Duration,
) -> Result<Duration> {
    let duration = humantime::parse_duration(value)
        .with_context(|| format!("{name} must be a duration such as 45s or 2m"))?;
    if !(minimum..=maximum).contains(&duration) {
        bail!(
            "{name} must be between {} and {}",
            humantime::format_duration(minimum),
            humantime::format_duration(maximum)
        );
    }
    Ok(duration)
}

pub fn parse_session_duration(name: &str, value: &str) -> Result<Duration> {
    parse_bounded_duration(
        name,
        value,
        Duration::from_millis(100),
        Duration::from_secs(7 * 24 * 60 * 60),
    )
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredCredentials {
    pub relay_url: Url,
    pub source_id: Uuid,
    pub source_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalPaths {
    pub config_dir: PathBuf,
    pub state_dir: PathBuf,
    pub runtime_dir: PathBuf,
}

impl LocalPaths {
    pub fn discover() -> Result<Self> {
        let project = ProjectDirs::from("com", "shellbell", "shellbell")
            .context("could not determine per-user Shellbell directories")?;
        let config_dir = env::var_os("SHELLBELL_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| project.config_dir().to_owned());
        let state_dir = env::var_os("SHELLBELL_STATE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| project.data_local_dir().to_owned());
        let runtime_dir = env::var_os("SHELLBELL_RUNTIME_DIR")
            .map(PathBuf::from)
            .or_else(valid_xdg_runtime_dir)
            .unwrap_or_else(|| PathBuf::from(format!("/tmp/shellbell-{}", effective_uid())));
        Ok(Self {
            config_dir,
            state_dir,
            runtime_dir,
        })
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn credentials_file(&self) -> PathBuf {
        self.config_dir.join("config.json")
    }

    pub fn database_file(&self) -> PathBuf {
        self.state_dir.join("activity.db")
    }

    pub fn socket_file(&self) -> PathBuf {
        self.runtime_dir.join("daemon.sock")
    }

    pub fn ensure_private_dirs(&self) -> Result<()> {
        ensure_private_dir(&self.config_dir)?;
        ensure_private_dir(&self.state_dir)?;
        ensure_private_dir(&self.runtime_dir)?;
        Ok(())
    }
}

fn valid_xdg_runtime_dir() -> Option<PathBuf> {
    let path = PathBuf::from(env::var_os("XDG_RUNTIME_DIR")?);
    let metadata = fs::metadata(&path).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if !metadata.is_dir() || metadata.uid() != effective_uid() || metadata.mode() & 0o022 != 0 {
            return None;
        }
    }
    Some(path.join("shellbell"))
}

fn effective_uid() -> u32 {
    #[cfg(unix)]
    {
        nix::unistd::Uid::effective().as_raw()
    }
    #[cfg(not(unix))]
    {
        0
    }
}

pub fn ensure_private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path).with_context(|| format!("could not create {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

pub fn load_credentials(path: &Path) -> Result<StoredCredentials> {
    let bytes = fs::read(path).with_context(|| {
        format!(
            "Shellbell is not paired; run `shellbell pair <relay-url>` (credentials expected at {})",
            path.display()
        )
    })?;
    serde_json::from_slice(&bytes)
        .context("Shellbell credentials are invalid; pair this machine again")
}

pub fn save_credentials(path: &Path, credentials: &StoredCredentials) -> Result<()> {
    let parent = path.parent().context("credentials path has no parent")?;
    ensure_private_dir(parent)?;
    let bytes = serde_json::to_vec_pretty(credentials)?;
    atomic_private_write(path, &bytes)
}

pub fn ensure_default_config(path: &Path) -> Result<bool> {
    if path.exists() {
        EffectiveConfig::load(path)?;
        return Ok(false);
    }
    let parent = path.parent().context("configuration path has no parent")?;
    ensure_private_dir(parent)?;
    atomic_private_write(path, DEFAULT_CONFIG.as_bytes())?;
    Ok(true)
}

fn atomic_private_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("file path has no parent")?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("shellbell"),
        Uuid::new_v4()
    ));
    #[cfg(unix)]
    let mut file = {
        use std::os::unix::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?
    };
    #[cfg(not(unix))]
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| -> Result<()> {
        file.write_all(bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_milestone_contract() {
        let config = EffectiveConfig::parse("").unwrap();
        assert_eq!(config.minimum_active, Duration::from_secs(120));
        assert_eq!(config.idle_for, Duration::from_secs(45));
        assert_eq!(config.targets, vec!["phone"]);
        assert_eq!(config.queue_limit, 100);
    }

    #[test]
    fn strict_validation_rejects_unknowns_and_bad_values() {
        assert!(EffectiveConfig::parse("surprise = true").is_err());
        assert!(EffectiveConfig::parse("[activity]\nminimum_active = \"zero\"").is_err());
        assert!(EffectiveConfig::parse("[delivery]\ntargets = [\"server\"]").is_err());
        assert!(EffectiveConfig::parse("[daemon]\nqueue_limit = 0").is_err());
    }

    #[test]
    fn credentials_stay_separate_and_private() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let credentials = StoredCredentials {
            relay_url: Url::parse("https://relay.example/").unwrap(),
            source_id: Uuid::new_v4(),
            source_token: "sb_src_private".into(),
        };
        save_credentials(&path, &credentials).unwrap();
        assert_eq!(load_credentials(&path).unwrap(), credentials);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn default_config_creation_is_atomic_and_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        assert!(ensure_default_config(&path).unwrap());
        assert!(!ensure_default_config(&path).unwrap());
        assert_eq!(
            EffectiveConfig::load(&path).unwrap(),
            EffectiveConfig::default()
        );
    }
}
