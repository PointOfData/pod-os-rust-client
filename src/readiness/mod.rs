//! Client-side AIP readiness polling and health probe construction.

mod gate;
mod health_probe;

pub use gate::{wait_for_actor_aip_ready, wait_for_gateway_aip_ready, ActorAIPReadinessConfig, GatewayReadinessProbe};
pub use health_probe::{
    actor_health_probe_succeeded, build_actor_health_probe_message,
    is_neural_memory_backed_for_health_probe,
};
