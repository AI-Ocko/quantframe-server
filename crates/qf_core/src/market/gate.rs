use std::sync::Arc;

use wf_market::gate::{install_gate, Gate, GateFuture};

use super::limiter::{self, BreakerOpen, Lane, Outcome};

/// Sends every wf-market call (sign-in, /me, orders, websocket setup) through the Trader lane.
pub struct TraderLaneGate;

impl Gate for TraderLaneGate {
    fn acquire(&self) -> GateFuture<'_, Result<(), String>> {
        Box::pin(async { limiter::global().try_acquire(Lane::Trader).await.map_err(|b| refusal(&b)) })
    }

    fn on_response(&self, status: u16, challenge: bool) {
        limiter::global().report(outcome(status, challenge));
    }

    fn on_transport_error(&self) {
        limiter::global().report(Outcome::TransportError);
    }
}

fn refusal(open: &BreakerOpen) -> String {
    format!("warframe.market unreachable: breaker open until {} UTC", open.until.format("%H:%M"))
}

/// `outcome_of` with the challenge flag standing in for `cf-mitigated: challenge`: wf-market's
/// `is_challenge` already applies the status rule, so every flagged response is a Challenge.
fn outcome(status: u16, challenge: bool) -> Outcome {
    let mut headers = reqwest::header::HeaderMap::new();
    if challenge {
        headers.insert("cf-mitigated", reqwest::header::HeaderValue::from_static("challenge"));
    }
    limiter::outcome_of(status, &headers)
}

pub fn install() {
    install_gate(Arc::new(TraderLaneGate));
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn refusal_names_the_reopen_time() {
        let open = BreakerOpen { until: chrono::Utc.with_ymd_and_hms(2026, 9, 26, 14, 5, 59).unwrap(), reason: "x".into() };
        assert_eq!(refusal(&open), "warframe.market unreachable: breaker open until 14:05 UTC");
    }

    #[test]
    fn the_challenge_flag_maps_onto_outcomes() {
        assert_eq!(outcome(200, false), Outcome::Ok);
        assert_eq!(outcome(403, false), Outcome::Ok);
        assert_eq!(outcome(403, true), Outcome::Challenge);
        assert_eq!(outcome(404, true), Outcome::Challenge);
        assert_eq!(outcome(502, true), Outcome::Challenge);
        assert_eq!(outcome(502, false), Outcome::TransportError);
        assert_eq!(outcome(429, false), Outcome::RateLimited);
    }
}
