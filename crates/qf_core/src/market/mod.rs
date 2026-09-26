//! warframe.market request budget shared by the trader, the collector and item refresh.

pub mod gate;
pub mod limiter;

/// warframe.market asks every client for a dedicated, descriptive User-Agent (spec P22).
pub const USER_AGENT: &str =
    concat!("quantframe-server/", env!("CARGO_PKG_VERSION"), " (+https://github.com/AI-Ocko/quantframe-server)");

/// A plain client for warframe.market that sends `USER_AGENT`.
pub fn http_client(timeout: std::time::Duration) -> reqwest::Client {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(reqwest::header::USER_AGENT, reqwest::header::HeaderValue::from_static(USER_AGENT));
    reqwest::Client::builder().timeout(timeout).default_headers(headers).build().expect("HTTP client")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// `serve_once` that answers 200 and hands back the raw request it received.
    async fn serve_once_capturing() -> (String, tokio::sync::oneshot::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let n = socket.read(&mut buf).await.unwrap();
            let _ = tx.send(String::from_utf8_lossy(&buf[..n]).to_lowercase());
            socket.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n").await.unwrap();
        });
        (format!("http://{addr}"), rx)
    }

    /// The collector, game-data and set-cache clients are all `http_client`.
    #[tokio::test]
    async fn every_client_sends_the_user_agent() {
        assert!(USER_AGENT.starts_with("quantframe-server/") && USER_AGENT.ends_with(" (+https://github.com/AI-Ocko/quantframe-server)"));
        let (url, request) = serve_once_capturing().await;
        http_client(std::time::Duration::from_secs(5)).get(url).send().await.unwrap();
        let request = request.await.unwrap();
        assert!(request.contains(&format!("user-agent: {}", USER_AGENT.to_lowercase())), "{request}");
    }
}
