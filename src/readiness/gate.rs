//! Poll until an actor or gateway answers an AIP health probe.

use std::time::{Duration, Instant};

use crate::{
    errors::GatewayDError,
    message::types::Message,
    readiness::health_probe::{actor_health_probe_succeeded, build_actor_health_probe_message},
};

/// Performs one AIP probe send. Callers wire this to their client stack.
#[allow(dead_code)]
pub type SendFunc =
    Box<dyn Fn(Message, &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Message, GatewayDError>> + Send>> + Send + Sync>;

/// Tunes the readiness polling loop. Zero fields fall back to defaults.
#[derive(Debug, Clone, Default)]
pub struct ActorAIPReadinessConfig {
    pub timeout: Duration,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
    pub required_consecutive: u32,
    pub success_interval: Duration,
}

impl ActorAIPReadinessConfig {
    fn normalized(self) -> Self {
        Self {
            timeout: if self.timeout.is_zero() {
                Duration::from_secs(60)
            } else {
                self.timeout
            },
            initial_backoff: if self.initial_backoff.is_zero() {
                Duration::from_secs(2)
            } else {
                self.initial_backoff
            },
            max_backoff: if self.max_backoff.is_zero() {
                Duration::from_secs(8)
            } else {
                self.max_backoff
            },
            required_consecutive: if self.required_consecutive == 0 {
                1
            } else {
                self.required_consecutive
            },
            success_interval: if self.success_interval.is_zero() {
                Duration::from_secs(2)
            } else {
                self.success_interval
            },
        }
    }
}

/// Names a known-stable anchor actor used to confirm a gateway route is serving AIP again.
#[derive(Debug, Clone, Default)]
pub struct GatewayReadinessProbe {
    pub probe_actor: String,
    pub probe_actor_type: String,
}

/// Polls until the named actor answers an AIP health probe, or the budget elapses.
pub async fn wait_for_actor_aip_ready<F, Fut>(
    send: F,
    actor_address: &str,
    from_address: &str,
    client_name: &str,
    actor_type: &str,
    rc: ActorAIPReadinessConfig,
) -> Result<(), GatewayDError>
where
    F: Fn(Message, String) -> Fut,
    Fut: std::future::Future<Output = Result<Message, GatewayDError>>,
{
    wait_for_aip_ready(
        send,
        actor_address,
        from_address,
        client_name,
        actor_type,
        rc,
    )
    .await
}

/// Polls until the stable anchor actor in probe answers an AIP health probe.
pub async fn wait_for_gateway_aip_ready<F, Fut>(
    send: F,
    probe: GatewayReadinessProbe,
    from_address: &str,
    client_name: &str,
    rc: ActorAIPReadinessConfig,
) -> Result<(), GatewayDError>
where
    F: Fn(Message, String) -> Fut,
    Fut: std::future::Future<Output = Result<Message, GatewayDError>>,
{
    if probe.probe_actor.is_empty() {
        return Err(GatewayDError::new(
            crate::errors::ErrCode::InvalidConfig,
            "gateway readiness probe: probe_actor is required",
        ));
    }
    wait_for_aip_ready(
        send,
        &probe.probe_actor,
        from_address,
        client_name,
        &probe.probe_actor_type,
        rc,
    )
    .await
}

async fn wait_for_aip_ready<F, Fut>(
    send: F,
    actor_address: &str,
    from_address: &str,
    client_name: &str,
    actor_type: &str,
    rc: ActorAIPReadinessConfig,
) -> Result<(), GatewayDError>
where
    F: Fn(Message, String) -> Fut,
    Fut: std::future::Future<Output = Result<Message, GatewayDError>>,
{
    let rc = rc.normalized();
    let deadline = Instant::now() + rc.timeout;
    let mut backoff = rc.initial_backoff;
    let mut last_msg = String::from("deadline exceeded");
    let mut consecutive = 0u32;
    let mut attempt = 0u32;

    while Instant::now() < deadline {
        attempt += 1;
        let probe_msg = build_actor_health_probe_message(
            actor_address,
            from_address,
            client_name,
            actor_type,
        );
        let label = format!("aip_ready_{actor_address}");
        let result = send(probe_msg, label).await;

        match &result {
            Ok(aip) if actor_health_probe_succeeded(None, Some(aip)) => {
                consecutive += 1;
                if consecutive >= rc.required_consecutive {
                    return Ok(());
                }
                backoff = rc.initial_backoff;
                tokio::time::sleep(rc.success_interval).await;
                continue;
            }
            Ok(aip) => {
                last_msg = format!("actor returned error: {}", aip.processing_message());
            }
            Err(e) => last_msg = e.message.clone(),
        }
        consecutive = 0;

        tokio::time::sleep(backoff).await;
        if backoff < rc.max_backoff {
            backoff = (backoff * 2).min(rc.max_backoff);
        }
    }

    Err(GatewayDError::new(
        crate::errors::ErrCode::GatewayError,
        format!(
            "actor {actor_address} not reachable over AIP within {:?}: {last_msg}",
            rc.timeout
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    };

    fn fast_config() -> ActorAIPReadinessConfig {
        ActorAIPReadinessConfig {
            timeout: Duration::from_millis(200),
            initial_backoff: Duration::from_millis(1),
            max_backoff: Duration::from_millis(2),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn succeeds_immediately() {
        let calls = Arc::new(AtomicU32::new(0));
        let calls2 = calls.clone();
        let send = move |_msg: Message, _label: String| {
            calls2.fetch_add(1, Ordering::SeqCst);
            async { Ok(Message::default()) }
        };
        wait_for_actor_aip_ready(
            send,
            "a@zeroth.pod-os.com",
            "c@zeroth.pod-os.com",
            "c",
            "socket",
            fast_config(),
        )
        .await
        .expect("success");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn gateway_uses_probe_actor() {
        let captured = Arc::new(std::sync::Mutex::new(String::new()));
        let captured2 = captured.clone();
        let send = move |msg: Message, _label: String| {
            *captured2.lock().unwrap() = msg.envelope.to.clone();
            async { Ok(Message::default()) }
        };
        wait_for_gateway_aip_ready(
            send,
            GatewayReadinessProbe {
                probe_actor: "test@zeroth.pod-os.com".into(),
                probe_actor_type: "neural_memory".into(),
            },
            "c@zeroth.pod-os.com",
            "c",
            fast_config(),
        )
        .await
        .expect("success");
        assert_eq!(*captured.lock().unwrap(), "test@zeroth.pod-os.com");
    }

    #[tokio::test]
    async fn retries_then_succeeds() {
        let calls = Arc::new(AtomicU32::new(0));
        let calls2 = calls.clone();
        let send = move |_msg: Message, _label: String| {
            let n = calls2.fetch_add(1, Ordering::SeqCst) + 1;
            async move {
                if n < 3 {
                    Err(GatewayDError::new(
                        crate::errors::ErrCode::ConnectionLost,
                        "connection to gateway was lost during request",
                    ))
                } else {
                    Ok(Message::default())
                }
            }
        };
        wait_for_actor_aip_ready(
            send,
            "a@zeroth.pod-os.com",
            "c@zeroth.pod-os.com",
            "c",
            "evolutionary-neural-memory",
            fast_config(),
        )
        .await
        .expect("eventual success");
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn deadline_exceeded() {
        let send = |_msg: Message, _label: String| async {
            Err(GatewayDError::new(
                crate::errors::ErrCode::ConnectionLost,
                "connection to gateway was lost during request",
            ))
        };
        assert!(
            wait_for_actor_aip_ready(
                send,
                "a@zeroth.pod-os.com",
                "c@zeroth.pod-os.com",
                "c",
                "socket",
                fast_config(),
            )
            .await
            .is_err()
        );
    }
}
