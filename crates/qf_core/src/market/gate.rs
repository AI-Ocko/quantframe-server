use std::sync::Arc;

use wf_market::gate::{install_gate, GateFuture, RequestGate};

use super::limiter::{self, Lane};

/// Sends every wf-market call (sign-in, /me, orders, websocket setup) through the Trader lane.
pub struct TraderLaneGate;

impl RequestGate for TraderLaneGate {
    fn acquire(&self) -> GateFuture<'_> {
        Box::pin(limiter::global().acquire(Lane::Trader))
    }

    fn on_status(&self, status: u16) {
        if status == 429 {
            limiter::global().report_429();
        }
    }
}

pub fn install() {
    install_gate(Arc::new(TraderLaneGate));
}
