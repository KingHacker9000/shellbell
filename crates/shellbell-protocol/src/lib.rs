//! Shared, wire-compatible Shellbell API models and validation contracts.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use url::Url;
use uuid::Uuid;

pub const MESSAGE_MAX: usize = 240;
pub const NAME_MAX: usize = 64;
pub const ENDPOINT_MAX: usize = 2048;
pub const KEY_MAX: usize = 512;
pub const PAIRING_CODE_LEN: usize = 9;
pub const ALLOWED_TAGS: [&str; 4] = ["phone", "pc", "mobile", "desktop"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiError {
    pub error: ErrorBody,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
}

impl ApiError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error: ErrorBody {
                code: code.into(),
                message: message.into(),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BootstrapStatusResponse {
    pub bootstrap_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BootstrapRequest {
    pub bootstrap_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionResponse {
    pub authenticated: bool,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PairingCreateRequest {
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PairingCreateResponse {
    pub id: Uuid,
    pub code: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PairingState {
    Pending,
    Approved,
    Rejected,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PairingPollResponse {
    pub status: PairingState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PairingView {
    pub id: Uuid,
    pub code: String,
    pub display_name: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PairingListResponse {
    pub pairings: Vec<PairingView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceView {
    pub id: Uuid,
    pub display_name: String,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceListResponse {
    pub sources: Vec<SourceView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RenameRequest {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PushSubscriptionInput {
    pub endpoint: String,
    pub p256dh: String,
    pub auth: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReceiverCreateRequest {
    pub name: String,
    pub tags: Vec<String>,
    pub subscription: PushSubscriptionInput,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReceiverUpdateRequest {
    pub name: Option<String>,
    pub tags: Option<Vec<String>>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReceiverView {
    pub id: Uuid,
    pub name: String,
    pub tags: Vec<String>,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReceiverListResponse {
    pub receivers: Vec<ReceiverView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RingRequest {
    pub event_id: Uuid,
    pub message: Option<String>,
    #[serde(default)]
    pub target_tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RingAcceptedResponse {
    pub event_id: Uuid,
    pub accepted_at: DateTime<Utc>,
    pub duplicate: bool,
    pub matched_receivers: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RingView {
    pub event_id: Uuid,
    pub source_id: Uuid,
    pub source_name: String,
    pub message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub target_tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RingListResponse {
    pub rings: Vec<RingView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SettingsResponse {
    pub history_retention_days: u32,
    pub vapid_public_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SettingsUpdateRequest {
    pub history_retention_days: u32,
}

pub fn validate_name(value: &str) -> Result<String, &'static str> {
    let value = value.trim();
    if value.is_empty() {
        return Err("name must not be empty");
    }
    if value.chars().count() > NAME_MAX {
        return Err("name is too long");
    }
    if value.chars().any(char::is_control) {
        return Err("name contains control characters");
    }
    Ok(value.to_owned())
}

pub fn validate_message(value: Option<&str>) -> Result<Option<String>, &'static str> {
    value
        .map(|v| {
            let v = v.trim();
            if v.chars().count() > MESSAGE_MAX {
                return Err("message is too long");
            }
            if v.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
                return Err("message contains control characters");
            }
            Ok(v.to_owned())
        })
        .transpose()
}

pub fn validate_tags(values: &[String]) -> Result<Vec<String>, &'static str> {
    let mut unique = BTreeSet::new();
    for value in values {
        if !ALLOWED_TAGS.contains(&value.as_str()) {
            return Err("unknown receiver tag");
        }
        unique.insert(value.clone());
    }
    Ok(unique.into_iter().collect())
}

pub fn validate_subscription(value: &PushSubscriptionInput) -> Result<(), &'static str> {
    if value.endpoint.len() > ENDPOINT_MAX
        || value.p256dh.len() > KEY_MAX
        || value.auth.len() > KEY_MAX
    {
        return Err("push subscription field is too long");
    }
    let endpoint = Url::parse(&value.endpoint).map_err(|_| "invalid push endpoint")?;
    if endpoint.scheme() != "https" {
        return Err("push endpoint must use HTTPS");
    }
    if value.p256dh.is_empty() || value.auth.is_empty() {
        return Err("push subscription keys are required");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_round_trip_and_enum_shape() {
        let request = RingRequest {
            event_id: Uuid::nil(),
            message: Some("hello".into()),
            target_tags: vec!["pc".into()],
        };
        let json = serde_json::to_string(&request).unwrap();
        assert_eq!(serde_json::from_str::<RingRequest>(&json).unwrap(), request);
        assert_eq!(
            serde_json::to_string(&PairingState::Approved).unwrap(),
            "\"approved\""
        );
    }

    #[test]
    fn validation_boundaries() {
        assert!(validate_name("x").is_ok());
        assert!(validate_name(&"x".repeat(NAME_MAX)).is_ok());
        assert!(validate_name(&"x".repeat(NAME_MAX + 1)).is_err());
        assert!(validate_message(Some(&"x".repeat(MESSAGE_MAX))).is_ok());
        assert!(validate_message(Some(&"x".repeat(MESSAGE_MAX + 1))).is_err());
        assert_eq!(
            validate_tags(&["pc".into(), "pc".into()]).unwrap(),
            vec!["pc"]
        );
        assert!(validate_tags(&["server".into()]).is_err());
    }

    #[test]
    fn push_subscription_requires_https_and_keys() {
        let mut input = PushSubscriptionInput {
            endpoint: "https://push.example/x".into(),
            p256dh: "key".into(),
            auth: "auth".into(),
        };
        assert!(validate_subscription(&input).is_ok());
        input.endpoint = "http://push.example/x".into();
        assert!(validate_subscription(&input).is_err());
    }
}
