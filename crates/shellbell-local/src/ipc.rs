use crate::activity::{Mode, Session, SessionState, ShellType};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{path::Path, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    time::timeout,
};
use uuid::Uuid;

pub const IPC_VERSION: u16 = 1;
pub const MAX_IPC_MESSAGE: usize = 8 * 1024;
pub const HOOK_TIMEOUT: Duration = Duration::from_millis(90);
pub const CLI_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IpcRequest {
    pub version: u16,
    pub request_id: Uuid,
    #[serde(flatten)]
    pub command: IpcCommand,
}

impl IpcRequest {
    pub fn new(command: IpcCommand) -> Self {
        Self {
            version: IPC_VERSION,
            request_id: Uuid::new_v4(),
            command,
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.version != IPC_VERSION {
            bail!(
                "unsupported local IPC version {}; expected {}",
                self.version,
                IPC_VERSION
            );
        }
        match &self.command {
            IpcCommand::SessionOpen { shell_pid, tty, .. } => {
                if *shell_pid == 0 {
                    bail!("shell_pid must be non-zero");
                }
                if tty
                    .as_ref()
                    .is_some_and(|value| value.len() > 256 || value.chars().any(char::is_control))
                {
                    bail!("tty metadata is invalid");
                }
            }
            IpcCommand::ArmPersistent {
                minimum_active_ms,
                idle_for_ms,
                targets,
                ..
            }
            | IpcCommand::ArmOnce {
                minimum_active_ms,
                idle_for_ms,
                targets,
                ..
            } => {
                validate_duration("minimum_active", *minimum_active_ms)?;
                validate_duration("idle_for", *idle_for_ms)?;
                shellbell_protocol::validate_tags(targets)
                    .map_err(|error| anyhow::anyhow!(error))?;
            }
            IpcCommand::SetLabel { label, .. } => {
                if let Some(label) = label {
                    shellbell_protocol::validate_name(label)
                        .map_err(|error| anyhow::anyhow!(error))?;
                }
            }
            IpcCommand::ManualRing {
                message, targets, ..
            } => {
                shellbell_protocol::validate_message(message.as_deref())
                    .map_err(|error| anyhow::anyhow!(error))?;
                shellbell_protocol::validate_tags(targets)
                    .map_err(|error| anyhow::anyhow!(error))?;
            }
            IpcCommand::SessionClose { .. }
            | IpcCommand::CommandStart { .. }
            | IpcCommand::PromptReady { .. }
            | IpcCommand::Disarm { .. }
            | IpcCommand::Status { .. }
            | IpcCommand::DaemonPing
            | IpcCommand::DaemonShutdown => {}
        }
        Ok(())
    }
}

fn validate_duration(name: &str, value: u64) -> Result<()> {
    if !(100..=7 * 24 * 60 * 60 * 1_000).contains(&value) {
        bail!("{name} duration is outside the supported range");
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum IpcCommand {
    SessionOpen {
        session_id: Uuid,
        shell_pid: u32,
        shell_type: ShellType,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tty: Option<String>,
    },
    CommandStart {
        session_id: Uuid,
    },
    PromptReady {
        session_id: Uuid,
    },
    SessionClose {
        session_id: Uuid,
    },
    ArmPersistent {
        session_id: Uuid,
        minimum_active_ms: u64,
        idle_for_ms: u64,
        targets: Vec<String>,
    },
    ArmOnce {
        session_id: Uuid,
        minimum_active_ms: u64,
        idle_for_ms: u64,
        targets: Vec<String>,
    },
    Disarm {
        session_id: Uuid,
    },
    SetLabel {
        session_id: Uuid,
        label: Option<String>,
    },
    Status {
        session_id: Uuid,
    },
    ManualRing {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<Uuid>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        targets: Vec<String>,
    },
    DaemonPing,
    DaemonShutdown,
}

impl IpcCommand {
    pub fn session_id(&self) -> Option<Uuid> {
        match self {
            Self::SessionOpen { session_id, .. }
            | Self::CommandStart { session_id }
            | Self::PromptReady { session_id }
            | Self::SessionClose { session_id }
            | Self::ArmPersistent { session_id, .. }
            | Self::ArmOnce { session_id, .. }
            | Self::Disarm { session_id }
            | Self::SetLabel { session_id, .. }
            | Self::Status { session_id } => Some(*session_id),
            Self::ManualRing { session_id, .. } => *session_id,
            Self::DaemonPing | Self::DaemonShutdown => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct IpcResponse {
    pub version: u16,
    pub request_id: Uuid,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<IpcResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl IpcResponse {
    pub fn success(request_id: Uuid, result: IpcResult) -> Self {
        Self {
            version: IPC_VERSION,
            request_id,
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    pub fn failure(request_id: Uuid, error: impl Into<String>) -> Self {
        Self {
            version: IPC_VERSION,
            request_id,
            ok: false,
            result: None,
            error: Some(error.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IpcResult {
    Acknowledged,
    Status { session: SessionStatus },
    RingQueued { event_id: Option<Uuid> },
    Pong { daemon: DaemonStatus },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionStatus {
    pub session_id: Uuid,
    pub shell_pid: u32,
    pub shell_type: ShellType,
    pub tty: Option<String>,
    pub mode: Mode,
    pub state: SessionState,
    pub burst_id: Option<Uuid>,
    pub accumulated_active_ms: u64,
    pub settling_deadline_wall_ms: Option<i64>,
    pub burst_notified: bool,
    pub label: Option<String>,
    pub targets: Vec<String>,
    pub last_seen_wall_ms: i64,
}

impl SessionStatus {
    pub fn from_session(session: &Session, now_mono_ms: u64) -> Self {
        Self {
            session_id: session.id,
            shell_pid: session.shell_pid,
            shell_type: session.shell_type,
            tty: session.tty.clone(),
            mode: session.mode,
            state: session.state,
            burst_id: session.burst_id,
            accumulated_active_ms: session.displayed_active_ms(now_mono_ms),
            settling_deadline_wall_ms: session.settling_deadline_wall_ms,
            burst_notified: session.burst_notified,
            label: session.label.clone(),
            targets: session.targets.clone(),
            last_seen_wall_ms: session.last_seen_wall_ms,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DaemonStatus {
    pub pending_events: u32,
    pub authorization_failures: u32,
    pub dropped_events: u64,
    pub active_sessions: u32,
}

pub fn encode_line<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec(value)?;
    if bytes.len() > MAX_IPC_MESSAGE {
        bail!("local IPC message exceeds {MAX_IPC_MESSAGE} bytes");
    }
    bytes.push(b'\n');
    Ok(bytes)
}

pub fn decode_request(bytes: &[u8]) -> Result<IpcRequest> {
    if bytes.len() > MAX_IPC_MESSAGE {
        bail!("local IPC message exceeds {MAX_IPC_MESSAGE} bytes");
    }
    let value: serde_json::Value =
        serde_json::from_slice(bytes).context("malformed local IPC JSON")?;
    validate_request_keys(&value)?;
    let request: IpcRequest = serde_json::from_slice(bytes).context("malformed local IPC JSON")?;
    request.validate()?;
    Ok(request)
}

fn validate_request_keys(value: &serde_json::Value) -> Result<()> {
    let object = value
        .as_object()
        .context("local IPC request must be a JSON object")?;
    let kind = object
        .get("type")
        .and_then(serde_json::Value::as_str)
        .context("local IPC request type is missing")?;
    let fields: &[&str] = match kind {
        "session_open" => &["session_id", "shell_pid", "shell_type", "tty"],
        "command_start" | "prompt_ready" | "session_close" | "disarm" | "status" => &["session_id"],
        "arm_persistent" | "arm_once" => {
            &["session_id", "minimum_active_ms", "idle_for_ms", "targets"]
        }
        "set_label" => &["session_id", "label"],
        "manual_ring" => &["session_id", "message", "targets"],
        "daemon_ping" | "daemon_shutdown" => &[],
        _ => bail!("unknown local IPC request type"),
    };
    for key in object.keys() {
        if !matches!(key.as_str(), "version" | "request_id" | "type")
            && !fields.contains(&key.as_str())
        {
            bail!("unknown field in local IPC request");
        }
    }
    Ok(())
}

pub async fn request(
    socket: &Path,
    request: &IpcRequest,
    operation_timeout: Duration,
) -> Result<IpcResponse> {
    let bytes = encode_line(request)?;
    timeout(operation_timeout, async {
        let mut stream = UnixStream::connect(socket)
            .await
            .with_context(|| format!("could not connect to daemon socket {}", socket.display()))?;
        stream.write_all(&bytes).await?;
        let mut response = Vec::new();
        stream
            .take((MAX_IPC_MESSAGE + 1) as u64)
            .read_to_end(&mut response)
            .await?;
        if response.len() > MAX_IPC_MESSAGE {
            bail!("daemon response exceeds {MAX_IPC_MESSAGE} bytes");
        }
        let value: IpcResponse = serde_json::from_slice(&response)
            .context("daemon returned malformed local IPC JSON")?;
        if value.version != IPC_VERSION || value.request_id != request.request_id {
            bail!("daemon returned a mismatched local IPC response");
        }
        if !value.ok {
            bail!(
                "{}",
                value
                    .error
                    .as_deref()
                    .unwrap_or("local daemon request failed")
            );
        }
        Ok(value)
    })
    .await
    .context("local daemon request timed out")?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_is_versioned_typed_and_contains_no_command_content_field() {
        let request = IpcRequest::new(IpcCommand::CommandStart {
            session_id: Uuid::nil(),
        });
        let json = String::from_utf8(encode_line(&request).unwrap()).unwrap();
        assert!(json.contains("\"version\":1"));
        assert!(json.contains("\"type\":\"command_start\""));
        assert!(!json.contains(&["command", "text"].join("_")));
        assert!(!json.contains(&["exit", "code"].join("_")));
        assert!(!json.contains("output"));
    }

    #[test]
    fn malformed_oversized_and_wrong_version_messages_are_rejected() {
        assert!(decode_request(b"not-json").is_err());
        assert!(decode_request(&vec![b'x'; MAX_IPC_MESSAGE + 1]).is_err());
        let mut request = IpcRequest::new(IpcCommand::DaemonPing);
        request.version = 99;
        assert!(decode_request(&serde_json::to_vec(&request).unwrap()).is_err());
        let unknown = br#"{"version":1,"request_id":"00000000-0000-0000-0000-000000000000","type":"daemon_ping","unexpected":"secret"}"#;
        assert!(decode_request(unknown).is_err());
    }

    #[test]
    fn invalid_fields_are_rejected() {
        let request = IpcRequest::new(IpcCommand::SessionOpen {
            session_id: Uuid::new_v4(),
            shell_pid: 0,
            shell_type: ShellType::Bash,
            tty: None,
        });
        assert!(request.validate().is_err());
        let request = IpcRequest::new(IpcCommand::ManualRing {
            session_id: None,
            message: None,
            targets: vec!["server".into()],
        });
        assert!(request.validate().is_err());
    }
}
