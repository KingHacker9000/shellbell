use async_trait::async_trait;
use serde::Serialize;
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;
use web_push::{
    ContentEncoding, IsahcWebPushClient, SubscriptionInfo, Urgency, VapidSignatureBuilder,
    WebPushClient, WebPushError, WebPushMessageBuilder,
};

const PUSH_TTL_SECONDS: u32 = 120;

#[derive(Clone, Debug)]
pub struct PushSubscription {
    pub endpoint: String,
    pub p256dh: String,
    pub auth: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeliveryOutcome {
    Delivered,
    PermanentFailure(String),
    TransientFailure(String),
}

#[async_trait]
pub trait PushDelivery: Send + Sync {
    async fn deliver(&self, subscription: &PushSubscription, payload: &[u8]) -> DeliveryOutcome;
}

pub struct VapidPushDelivery {
    private_key: String,
    subject: String,
    timeout: Duration,
}

impl VapidPushDelivery {
    pub fn new(private_key: String, subject: String, timeout: Duration) -> Self {
        Self {
            private_key,
            subject,
            timeout,
        }
    }
}

#[async_trait]
impl PushDelivery for VapidPushDelivery {
    async fn deliver(&self, subscription: &PushSubscription, payload: &[u8]) -> DeliveryOutcome {
        let info = SubscriptionInfo::new(
            &subscription.endpoint,
            &subscription.p256dh,
            &subscription.auth,
        );
        let mut signature = match VapidSignatureBuilder::from_base64(&self.private_key, &info) {
            Ok(builder) => builder,
            Err(error) => {
                return DeliveryOutcome::TransientFailure(error.short_description().into());
            }
        };
        signature.add_claim("sub", self.subject.as_str());
        let signature = match signature.build() {
            Ok(signature) => signature,
            Err(error) => {
                return DeliveryOutcome::TransientFailure(error.short_description().into());
            }
        };
        let mut builder = WebPushMessageBuilder::new(&info);
        builder.set_payload(ContentEncoding::Aes128Gcm, payload);
        builder.set_ttl(PUSH_TTL_SECONDS);
        builder.set_urgency(Urgency::High);
        builder.set_vapid_signature(signature);
        let message = match builder.build() {
            Ok(message) => message,
            Err(error) => return classify(error),
        };
        let client = match IsahcWebPushClient::new() {
            Ok(client) => client,
            Err(error) => return classify(error),
        };
        match tokio::time::timeout(self.timeout, client.send(message)).await {
            Ok(Ok(())) => DeliveryOutcome::Delivered,
            Ok(Err(error)) => classify(error),
            Err(_) => DeliveryOutcome::TransientFailure("timeout".into()),
        }
    }
}

fn classify(error: WebPushError) -> DeliveryOutcome {
    let diagnostic = error.short_description().to_owned();
    match error {
        WebPushError::EndpointNotValid(_)
        | WebPushError::EndpointNotFound(_)
        | WebPushError::InvalidUri
        | WebPushError::InvalidCryptoKeys
        | WebPushError::MissingCryptoKeys => DeliveryOutcome::PermanentFailure(diagnostic),
        _ => DeliveryOutcome::TransientFailure(diagnostic),
    }
}

#[derive(Default, Clone)]
pub struct FakePushDelivery {
    pub attempts: Arc<Mutex<Vec<FakeAttempt>>>,
    pub outcome: Arc<Mutex<Option<DeliveryOutcome>>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct FakeAttempt {
    pub endpoint_host_hint: String,
    pub payload: Vec<u8>,
}

#[async_trait]
impl PushDelivery for FakePushDelivery {
    async fn deliver(&self, subscription: &PushSubscription, payload: &[u8]) -> DeliveryOutcome {
        let hint = subscription
            .endpoint
            .split('/')
            .nth(2)
            .unwrap_or("invalid")
            .to_owned();
        self.attempts.lock().await.push(FakeAttempt {
            endpoint_host_hint: hint,
            payload: payload.to_vec(),
        });
        self.outcome
            .lock()
            .await
            .clone()
            .unwrap_or(DeliveryOutcome::Delivered)
    }
}
