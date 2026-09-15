use std::{future::Future, pin::Pin, time::Duration};

use super::orders::{parse_orders_response, V2Order};
use crate::market::limiter::{Lane, Limiter};

pub const WFM_API_V2: &str = "https://api.warframe.market/v2";
pub const MAX_RETRIES: u32 = 2;

#[derive(Debug, Clone, PartialEq)]
pub enum FetchError {
    NotFound,
    RateLimited,
    Transient(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::NotFound => write!(f, "not found"),
            FetchError::RateLimited => write!(f, "rate limited"),
            FetchError::Transient(message) => write!(f, "{message}"),
        }
    }
}

pub type FetchFuture<'a> = Pin<Box<dyn Future<Output = Result<Vec<V2Order>, FetchError>> + Send + 'a>>;

pub trait OrderSource: Send + Sync {
    fn fetch<'a>(&'a self, slug: &'a str) -> FetchFuture<'a>;
}

/// Unauthenticated client for the public `GET /v2/orders/item/{slug}` (amendment B2).
pub struct HttpOrderSource {
    http: reqwest::Client,
    base_url: String,
}

impl HttpOrderSource {
    pub fn new(http: reqwest::Client, base_url: impl Into<String>) -> Self {
        Self { http, base_url: base_url.into() }
    }
}

impl OrderSource for HttpOrderSource {
    fn fetch<'a>(&'a self, slug: &'a str) -> FetchFuture<'a> {
        Box::pin(async move {
            let url = format!("{}/orders/item/{}", self.base_url, slug);
            let response = self
                .http
                .get(&url)
                .header("Platform", "pc")
                .header("Language", "en")
                .header("Crossplay", "true")
                .send()
                .await
                .map_err(|e| FetchError::Transient(e.to_string()))?;
            match response.status().as_u16() {
                200 => {}
                404 => return Err(FetchError::NotFound),
                429 => return Err(FetchError::RateLimited),
                code => return Err(FetchError::Transient(format!("HTTP {code} for {url}"))),
            }
            let body = response.text().await.map_err(|e| FetchError::Transient(e.to_string()))?;
            parse_orders_response(&body).map_err(|e| FetchError::Transient(e.message))
        })
    }
}

/// Takes a limiter token for every attempt. Retries everything except 404 at most twice with jitter
/// (amendment B10). A 429 also pauses the whole limiter.
pub async fn fetch_with_retries(
    source: &dyn OrderSource,
    limiter: &Limiter,
    lane: Lane,
    slug: &str,
) -> Result<Vec<V2Order>, FetchError> {
    let mut retries = 0;
    loop {
        limiter.acquire(lane).await;
        match source.fetch(slug).await {
            Ok(orders) => return Ok(orders),
            Err(FetchError::NotFound) => return Err(FetchError::NotFound),
            Err(error) => {
                if error == FetchError::RateLimited {
                    limiter.report_429();
                }
                if retries >= MAX_RETRIES {
                    return Err(error);
                }
                retries += 1;
                tokio::time::sleep(jitter()).await;
            }
        }
    }
}

fn jitter() -> Duration {
    let mut bytes = [0u8; 2];
    let _ = getrandom::getrandom(&mut bytes);
    Duration::from_millis(500 + u64::from(u16::from_le_bytes(bytes)) % 1000)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    const SMALL: &str = include_str!("../../tests/fixtures/orders_small.json");

    pub(crate) struct ScriptedSource {
        pub responses: Mutex<VecDeque<Result<Vec<V2Order>, FetchError>>>,
        pub calls: AtomicUsize,
    }

    impl ScriptedSource {
        pub(crate) fn new(responses: Vec<Result<Vec<V2Order>, FetchError>>) -> Self {
            Self { responses: Mutex::new(responses.into()), calls: AtomicUsize::new(0) }
        }
    }

    impl OrderSource for ScriptedSource {
        fn fetch<'a>(&'a self, _slug: &'a str) -> FetchFuture<'a> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                self.responses
                    .lock()
                    .unwrap()
                    .pop_front()
                    .unwrap_or_else(|| Err(FetchError::Transient("script exhausted".into())))
            })
        }
    }

    fn transient() -> Result<Vec<V2Order>, FetchError> {
        Err(FetchError::Transient("HTTP 502".into()))
    }

    #[tokio::test(start_paused = true)]
    async fn transient_errors_are_retried_twice() {
        let limiter = Limiter::new(1000);
        let source = ScriptedSource::new(vec![transient(), transient(), Ok(vec![])]);
        assert_eq!(fetch_with_retries(&source, &limiter, Lane::Cold, "x").await, Ok(vec![]));
        assert_eq!(source.calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn gives_up_after_two_retries() {
        let limiter = Limiter::new(1000);
        let source = ScriptedSource::new(vec![transient(), transient(), transient(), Ok(vec![])]);
        assert!(matches!(fetch_with_retries(&source, &limiter, Lane::Hot, "x").await, Err(FetchError::Transient(_))));
        assert_eq!(source.calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn not_found_is_not_retried() {
        let limiter = Limiter::new(1000);
        let source = ScriptedSource::new(vec![Err(FetchError::NotFound), Ok(vec![])]);
        assert_eq!(fetch_with_retries(&source, &limiter, Lane::Cold, "x").await, Err(FetchError::NotFound));
        assert_eq!(source.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn rate_limited_pauses_the_limiter_then_retries() {
        let limiter = Limiter::new(1000);
        let source = ScriptedSource::new(vec![Err(FetchError::RateLimited), Ok(vec![])]);
        let start = tokio::time::Instant::now();
        assert_eq!(fetch_with_retries(&source, &limiter, Lane::Cold, "x").await, Ok(vec![]));
        assert_eq!(limiter.snapshot().rate_limited_total, 1);
        assert!(start.elapsed() >= std::time::Duration::from_secs(5));
    }

    async fn serve_once(status_line: &'static str, body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let _ = socket.read(&mut buf).await;
            let response = format!(
                "HTTP/1.1 {status_line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn http_source_maps_status_codes() {
        let http = reqwest::Client::new();
        let ok = HttpOrderSource::new(http.clone(), serve_once("200 OK", SMALL).await);
        assert_eq!(ok.fetch("item").await.unwrap().len(), 7);
        let missing = HttpOrderSource::new(http.clone(), serve_once("404 Not Found", "{}").await);
        assert_eq!(missing.fetch("item").await, Err(FetchError::NotFound));
        let limited = HttpOrderSource::new(http.clone(), serve_once("429 Too Many Requests", "").await);
        assert_eq!(limited.fetch("item").await, Err(FetchError::RateLimited));
        let broken = HttpOrderSource::new(http.clone(), serve_once("502 Bad Gateway", "").await);
        assert!(matches!(broken.fetch("item").await, Err(FetchError::Transient(_))));
        let garbage = HttpOrderSource::new(http, serve_once("200 OK", "not json").await);
        assert!(matches!(garbage.fetch("item").await, Err(FetchError::Transient(_))));
    }
}
