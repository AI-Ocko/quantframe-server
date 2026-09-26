//! quantframe-server patch: a process-wide gate that every `call_api` request passes.
//! See PATCHES.md, changes 4 and 5.

use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, OnceLock},
};

pub type GateFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait Gate: Send + Sync {
    /// Resolves when the request may be sent; `Err(text)` = do not send, `text` becomes the error content.
    fn acquire(&self) -> GateFuture<'_, Result<(), String>>;
    /// Receives every response: its HTTP status and whether it looks like a Cloudflare challenge.
    fn on_response(&self, status: u16, challenge: bool);
    /// A request got no response (connect error, timeout, ...).
    fn on_transport_error(&self);
}

/// A Cloudflare challenge: `cf-mitigated: challenge` on any status >= 400, or a `text/html`
/// content type on a 403 or 503 (the API itself answers JSON; Cloudflare's 502/52x origin-error
/// pages are HTML too and are not challenges). Mirrors quantframe-server's `outcome_of`.
pub fn is_challenge(status: u16, headers: &reqwest::header::HeaderMap) -> bool {
    let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).unwrap_or("");
    (status >= 400 && header("cf-mitigated").eq_ignore_ascii_case("challenge"))
        || (matches!(status, 403 | 503) && header("content-type").to_ascii_lowercase().starts_with("text/html"))
}

static GATE: OnceLock<Arc<dyn Gate>> = OnceLock::new();

/// Installs the gate for every client in this process. Returns `false` if one was already installed.
pub fn install_gate(gate: Arc<dyn Gate>) -> bool {
    GATE.set(gate).is_ok()
}

pub(crate) fn installed() -> Option<&'static Arc<dyn Gate>> {
    GATE.get()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{client::Client, enums::ApiVersion, errors::ApiError};
    use reqwest::{
        header::{HeaderMap, HeaderValue},
        Method,
    };
    use std::sync::Mutex;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    /// The gate is process-wide, so every test shares one recorder and runs one at a time.
    #[derive(Default)]
    struct Recorder {
        acquired: Mutex<usize>,
        refuse: Mutex<Option<String>>,
        responses: Mutex<Vec<(u16, bool)>>,
        transport_errors: Mutex<usize>,
    }

    impl Gate for Recorder {
        fn acquire(&self) -> GateFuture<'_, Result<(), String>> {
            Box::pin(async move {
                *self.acquired.lock().unwrap() += 1;
                match self.refuse.lock().unwrap().clone() {
                    Some(text) => Err(text),
                    None => Ok(()),
                }
            })
        }
        fn on_response(&self, status: u16, challenge: bool) {
            self.responses.lock().unwrap().push((status, challenge));
        }
        fn on_transport_error(&self) {
            *self.transport_errors.lock().unwrap() += 1;
        }
    }

    static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    static RECORDER: OnceLock<Arc<Recorder>> = OnceLock::new();

    fn recorder() -> Arc<Recorder> {
        RECORDER
            .get_or_init(|| {
                let r = Arc::new(Recorder::default());
                assert!(install_gate(r.clone()));
                r
            })
            .clone()
    }

    fn reset(r: &Recorder) {
        *r.acquired.lock().unwrap() = 0;
        *r.refuse.lock().unwrap() = None;
        r.responses.lock().unwrap().clear();
        *r.transport_errors.lock().unwrap() = 0;
    }

    async fn probe(addr: std::net::SocketAddr) -> Result<(serde_json::Value, HeaderMap, crate::errors::RequestError), ApiError> {
        let version = ApiVersion::Custom(format!("http://{}", addr), String::new());
        Client::new()
            .call_api::<serde_json::Value>(version, Method::GET, "/probe", "GET:probe", None, None)
            .await
    }

    #[tokio::test]
    async fn every_api_call_passes_the_installed_gate() {
        let _serial = SERIAL.lock().await;
        let recorder = recorder();
        reset(&recorder);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let _ = socket.read(&mut buf).await;
            socket
                .write_all(b"HTTP/1.1 429 Too Many Requests\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
                .await
                .unwrap();
        });

        let result = probe(addr).await;

        assert!(matches!(result, Err(ApiError::TooManyRequests(_))));
        assert_eq!(*recorder.acquired.lock().unwrap(), 1);
        assert_eq!(*recorder.responses.lock().unwrap(), vec![(429, false)]);
        assert_eq!(*recorder.transport_errors.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn a_closed_gate_returns_request_error_without_sending() {
        let _serial = SERIAL.lock().await;
        let recorder = recorder();
        reset(&recorder);
        let text = "warframe.market unreachable: breaker open until 14:45 UTC".to_string();
        *recorder.refuse.lock().unwrap() = Some(text.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let result = probe(addr).await;

        match result {
            Err(ApiError::RequestError(e)) => assert_eq!(e.content, text),
            other => panic!("expected RequestError, got {:?}", other.map(|_| ())),
        }
        let accepted = tokio::time::timeout(std::time::Duration::from_millis(200), listener.accept()).await;
        assert!(accepted.is_err(), "the closed gate let a request through");
        assert!(recorder.responses.lock().unwrap().is_empty());
        assert_eq!(*recorder.transport_errors.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn transport_error_is_reported() {
        let _serial = SERIAL.lock().await;
        let recorder = recorder();
        reset(&recorder);
        let addr = TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap();

        let result = probe(addr).await;

        assert!(matches!(result, Err(ApiError::RequestError(_))));
        assert_eq!(*recorder.transport_errors.lock().unwrap(), 1);
        assert!(recorder.responses.lock().unwrap().is_empty());
    }

    #[test]
    fn challenge_responses_are_flagged() {
        let headers = |pairs: &[(&'static str, &'static str)]| {
            let mut h = HeaderMap::new();
            for (k, v) in pairs {
                h.insert(*k, HeaderValue::from_static(v));
            }
            h
        };
        let html = headers(&[("content-type", "text/html; charset=UTF-8")]);
        let json = headers(&[("content-type", "application/json")]);
        assert!(is_challenge(403, &html));
        assert!(is_challenge(503, &html));
        assert!(!is_challenge(502, &html));
        assert!(!is_challenge(200, &html));
        assert!(is_challenge(404, &headers(&[("cf-mitigated", "challenge"), ("content-type", "application/json")])));
        assert!(is_challenge(403, &headers(&[("cf-mitigated", "Challenge")])));
        assert!(!is_challenge(200, &headers(&[("cf-mitigated", "challenge")])));
        assert!(!is_challenge(403, &json));
        assert!(!is_challenge(403, &headers(&[])));
    }

    #[tokio::test]
    async fn an_html_403_reaches_the_gate_flagged() {
        let _serial = SERIAL.lock().await;
        let recorder = recorder();
        reset(&recorder);
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let _ = socket.read(&mut buf).await;
            let body = "<html>Just a moment...</html>";
            let reply = format!(
                "HTTP/1.1 403 Forbidden\r\ncontent-type: text/html; charset=UTF-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            socket.write_all(reply.as_bytes()).await.unwrap();
        });

        let result = probe(addr).await;

        assert!(result.is_err());
        assert_eq!(*recorder.responses.lock().unwrap(), vec![(403, true)]);
    }
}
