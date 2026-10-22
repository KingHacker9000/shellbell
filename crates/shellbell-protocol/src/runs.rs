use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Explicit collector metadata. Never deserialize a native agent payload into this type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RunBeacon {
    pub collector: String,
    pub machine: String,
    pub run_id: String,
    pub state: RunState,
    pub observed_at: DateTime<Utc>,
    #[serde(default)]
    pub heartbeat: bool,
    #[serde(default = "default_stale")]
    pub stale_after_seconds: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job: Option<SlurmJobMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SlurmJobMetadata {
    pub name: String,
    pub partition: String,
    pub elapsed: String,
    pub time_limit: String,
    pub cpus: Option<u32>,
    pub min_memory: String,
    pub tres_per_node: String,
    pub start_time: String,
    pub node_reason: String,
}
fn default_stale() -> u32 {
    3600
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Started,
    Running,
    Queued,
    WaitingInput,
    WaitingApproval,
    Finished,
    Failed,
    Cancelled,
    Ended,
    Unknown,
    NeedsReauth,
}
impl RunState {
    pub fn is_active(self) -> bool {
        matches!(self, Self::Started | Self::Running | Self::Queued)
    }
    pub fn needs_attention(self) -> bool {
        matches!(
            self,
            Self::WaitingInput
                | Self::WaitingApproval
                | Self::Finished
                | Self::Failed
                | Self::Cancelled
                | Self::NeedsReauth
        )
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Running => "running",
            Self::Queued => "queued",
            Self::WaitingInput => "needs your answer",
            Self::WaitingApproval => "needs approval",
            Self::Finished => "finished",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Ended => "session ended",
            Self::Unknown => "status unknown",
            Self::NeedsReauth => "needs SSH re-authentication",
        }
    }
}
impl RunBeacon {
    pub fn validate(&self) -> Result<(), &'static str> {
        if let Some(job) = &self.job {
            if self.collector != "slurm" {
                return Err("job metadata is only allowed for Slurm");
            }
            for value in [
                &job.name,
                &job.partition,
                &job.elapsed,
                &job.time_limit,
                &job.min_memory,
                &job.tres_per_node,
                &job.start_time,
                &job.node_reason,
            ] {
                if value.chars().count() > 256 || value.chars().any(char::is_control) {
                    return Err("invalid Slurm display metadata");
                }
            }
            if job.cpus.is_some_and(|n| n > 1_000_000) {
                return Err("invalid CPU allocation");
            }
        }
        for value in [&self.collector, &self.machine, &self.run_id] {
            if value.is_empty()
                || value.len() > 128
                || !value
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "._:@+-".contains(c))
            {
                return Err("run identifiers must be 1-128 ASCII identifier characters");
            }
        }
        if !(60..=86400).contains(&self.stale_after_seconds) {
            return Err("stale interval must be 60-86400 seconds");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn beacon() -> RunBeacon {
        RunBeacon {
            collector: "custom-adapter".into(),
            machine: "host".into(),
            run_id: "session:123".into(),
            state: RunState::WaitingInput,
            observed_at: Utc::now(),
            heartbeat: false,
            stale_after_seconds: 3600,
            job: None,
        }
    }
    #[test]
    fn adapters_are_extensible_but_identifiers_cannot_contain_text_or_paths() {
        let mut b = beacon();
        assert!(b.validate().is_ok());
        for invalid in ["", "/private/path", "question text?", "line\nfeed"] {
            b.run_id = invalid.into();
            assert!(b.validate().is_err());
        }
    }
    #[test]
    fn beacon_unknown_fields_are_rejected() {
        let mut b = serde_json::to_value(beacon()).unwrap();
        b["prompt"] = serde_json::json!("private");
        assert!(serde_json::from_value::<RunBeacon>(b).is_err());
    }
    #[test]
    fn slurm_details_are_an_explicit_bounded_allowlist() {
        let mut b = beacon();
        b.collector = "slurm".into();
        b.job = Some(SlurmJobMetadata {
            name: "training-baseline".into(),
            partition: "gpu".into(),
            elapsed: "12:31".into(),
            time_limit: "2:00:00".into(),
            cpus: Some(4),
            min_memory: "48G".into(),
            tres_per_node: "gres/gpu:example:1".into(),
            start_time: "N/A".into(),
            node_reason: "(Dependency)".into(),
        });
        assert!(b.validate().is_ok());
        let mut payload = serde_json::to_value(&b).unwrap();
        payload["job"]["command"] = serde_json::json!("private command");
        assert!(serde_json::from_value::<RunBeacon>(payload).is_err());
        b.collector = "claude".into();
        assert!(b.validate().is_err());
        b.collector = "slurm".into();
        b.job.as_mut().unwrap().name = "line\nfeed".into();
        assert!(b.validate().is_err());
    }
    #[test]
    fn freshness_interval_is_bounded() {
        let mut b = beacon();
        for seconds in [0, 59, 86401, u32::MAX] {
            b.stale_after_seconds = seconds;
            assert!(b.validate().is_err());
        }
        for seconds in [60, 3600, 86400] {
            b.stale_after_seconds = seconds;
            assert!(b.validate().is_ok());
        }
    }
    #[test]
    fn question_approval_completion_and_liveness_are_distinct() {
        for state in [
            RunState::WaitingInput,
            RunState::WaitingApproval,
            RunState::Failed,
            RunState::Finished,
        ] {
            assert!(state.needs_attention());
            assert!(!state.is_active());
        }
        for state in [RunState::Running, RunState::Started, RunState::Queued] {
            assert!(state.is_active());
            assert!(!state.needs_attention());
        }
        assert!(!RunState::Unknown.needs_attention());
        assert!(!RunState::Ended.is_active());
    }
}
