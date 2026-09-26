use std::{future::Future, pin::Pin, time::Duration};

use super::orders::{parse_orders_response, V2Order};
use crate::market::limiter::{outcome_of, Lane, Limiter, Outcome};

pub const WFM_API_V2: &str = "https://api.warframe.market/v2";
pub const MAX_RETRIES: u32 = 2;

#[derive(Debug, Clone, PartialEq)]
pub enum FetchError {
    NotFound,
    RateLimited,
    /// A Cloudflare challenge: warframe.market is blocking this IP (spec P21).
    Blocked,
    /// A transport failure or a 5xx.
    Transient(String),
    /// A response arrived but is unusable (another 4xx, a body that does not parse).
    Invalid(String),
}

impl FetchError {
    /// What this attempt tells the breaker: a 404 or an unusable answer is still an answer.
    pub fn outcome(&self) -> Outcome {
        match self {
            FetchError::NotFound | FetchError::Invalid(_) => Outcome::Ok,
            FetchError::RateLimited => Outcome::RateLimited,
            FetchError::Blocked => Outcome::Challenge,
            FetchError::Transient(_) => Outcome::TransportError,
        }
    }
}

/// Sends a warframe.market GET and returns the body of a 200; every other answer (or none) is a `FetchError`.
pub(crate) async fn get_body(request: reqwest::RequestBuilder) -> Result<String, FetchError> {
    let response = request.send().await.map_err(|e| FetchError::Transient(e.to_string()))?;
    let status = response.status().as_u16();
    let what = || format!("HTTP {status} for {}", response.url());
    match outcome_of(status, response.headers()) {
        Outcome::Challenge => return Err(FetchError::Blocked),
        Outcome::RateLimited => return Err(FetchError::RateLimited),
        Outcome::TransportError => return Err(FetchError::Transient(what())),
        Outcome::Ok if status == 404 => return Err(FetchError::NotFound),
        Outcome::Ok if status != 200 => return Err(FetchError::Invalid(what())),
        Outcome::Ok => {}
    }
    response.text().await.map_err(|e| FetchError::Transient(e.to_string()))
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::NotFound => write!(f, "not found"),
            FetchError::RateLimited => write!(f, "rate limited"),
            FetchError::Blocked => write!(f, "blocked by a Cloudflare challenge"),
            FetchError::Transient(message) | FetchError::Invalid(message) => write!(f, "{message}"),
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
            let request = self.http.get(&url).header("Platform", "pc").header("Language", "en").header("Crossplay", "true");
            let body = get_body(request).await?;
            parse_orders_response(&body).map_err(|e| FetchError::Invalid(e.message))
        })
    }
}

/// Takes a limiter token for every attempt and reports its outcome to the breaker (spec P21).
/// Retries a transient or unusable answer at most twice with jitter (amendment B10); a 404, a
/// block or a 429 is final, and the last two have already tripped the breaker.
pub async fn fetch_with_retries(
    source: &dyn OrderSource,
    limiter: &Limiter,
    lane: Lane,
    slug: &str,
) -> Result<Vec<V2Order>, FetchError> {
    let mut retries = 0;
    loop {
        limiter.acquire(lane).await;
        let result = source.fetch(slug).await;
        limiter.report(result.as_ref().err().map_or(Outcome::Ok, FetchError::outcome));
        match result {
            Ok(orders) => return Ok(orders),
            Err(error @ (FetchError::NotFound | FetchError::Blocked | FetchError::RateLimited)) => return Err(error),
            Err(error) => {
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
    async fn rate_limited_is_not_retried_and_trips() {
        let limiter = Limiter::new(1000);
        let source = ScriptedSource::new(vec![Err(FetchError::RateLimited), Ok(vec![])]);
        assert_eq!(fetch_with_retries(&source, &limiter, Lane::Cold, "x").await, Err(FetchError::RateLimited));
        assert_eq!(source.calls.load(Ordering::SeqCst), 1);
        let snapshot = limiter.snapshot();
        assert_eq!((snapshot.rate_limited_total, snapshot.breaker.state.as_str()), (1, "open"));
    }

    #[tokio::test(start_paused = true)]
    async fn blocked_is_not_retried_and_trips_the_limiter() {
        let limiter = Limiter::new(1000);
        let source = ScriptedSource::new(vec![Err(FetchError::Blocked), Ok(vec![])]);
        assert_eq!(fetch_with_retries(&source, &limiter, Lane::Hot, "x").await, Err(FetchError::Blocked));
        assert_eq!(source.calls.load(Ordering::SeqCst), 1);
        let breaker = limiter.snapshot().breaker;
        assert_eq!(breaker.state, "open");
        assert!(breaker.reason.unwrap().contains("challenge"));
    }

    const JSON: &[(&str, &str)] = &[("content-type", "application/json")];

    async fn serve_once(status_line: &'static str, headers: &'static [(&'static str, &'static str)], body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let _ = socket.read(&mut buf).await;
            let headers: String = headers.iter().map(|(k, v)| format!("{k}: {v}\r\n")).collect();
            let response = format!(
                "HTTP/1.1 {status_line}\r\n{headers}content-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn http_source_maps_status_codes() {
        let http = reqwest::Client::new();
        let ok = HttpOrderSource::new(http.clone(), serve_once("200 OK", JSON, SMALL).await);
        assert_eq!(ok.fetch("item").await.unwrap().len(), 7);
        let missing = HttpOrderSource::new(http.clone(), serve_once("404 Not Found", JSON, "{}").await);
        assert_eq!(missing.fetch("item").await, Err(FetchError::NotFound));
        let limited = HttpOrderSource::new(http.clone(), serve_once("429 Too Many Requests", JSON, "").await);
        assert_eq!(limited.fetch("item").await, Err(FetchError::RateLimited));
        let challenge = &[("content-type", "text/html; charset=UTF-8"), ("cf-mitigated", "challenge")];
        let blocked = HttpOrderSource::new(http.clone(), serve_once("403 Forbidden", challenge, "Just a moment...").await);
        assert_eq!(blocked.fetch("item").await, Err(FetchError::Blocked));
        let broken = HttpOrderSource::new(http.clone(), serve_once("502 Bad Gateway", JSON, "").await);
        assert!(matches!(broken.fetch("item").await, Err(FetchError::Transient(_))));
        let refused = HttpOrderSource::new(http.clone(), serve_once("400 Bad Request", JSON, "{}").await);
        assert!(matches!(refused.fetch("item").await, Err(FetchError::Invalid(_))));
        let garbage = HttpOrderSource::new(http, serve_once("200 OK", JSON, "not json").await);
        assert!(matches!(garbage.fetch("item").await, Err(FetchError::Invalid(_))));
    }
}
