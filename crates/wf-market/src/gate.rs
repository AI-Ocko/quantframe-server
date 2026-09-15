//! quantframe-server patch: a process-wide gate that every `call_api` request passes.
//! See PATCHES.md, change 4.

use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, OnceLock},
};

pub type GateFuture<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

pub trait RequestGate: Send + Sync {
    /// Resolves when the request may be sent.
    fn acquire(&self) -> GateFuture<'_>;
    /// Receives the HTTP status of every response.
    fn on_status(&self, status: u16);
}

static GATE: OnceLock<Arc<dyn RequestGate>> = OnceLock::new();

/// Installs the gate for every client in this process. Returns `false` if one was already installed.
pub fn install_gate(gate: Arc<dyn RequestGate>) -> bool {
    GATE.set(gate).is_ok()
}

pub(crate) fn installed() -> Option<&'static Arc<dyn RequestGate>> {
    GATE.get()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{client::Client, enums::ApiVersion};
    use reqwest::Method;
    use std::sync::Mutex;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    #[derive(Default)]
    struct Recorder {
        acquired: Mutex<usize>,
        statuses: Mutex<Vec<u16>>,
    }

    impl RequestGate for Recorder {
        fn acquire(&self) -> GateFuture<'_> {
            Box::pin(async move {
                *self.acquired.lock().unwrap() += 1;
            })
        }
        fn on_status(&self, status: u16) {
            self.statuses.lock().unwrap().push(status);
        }
    }

    #[tokio::test]
    async fn every_api_call_passes_the_installed_gate() {
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

        let recorder = Arc::new(Recorder::default());
        assert!(install_gate(recorder.clone()));
        let client = Client::new();
        let version = ApiVersion::Custom(format!("http://{}", addr), String::new());
        let result = client
            .call_api::<serde_json::Value>(version, Method::GET, "/probe", "GET:probe", None, None)
            .await;

        assert!(result.is_err());
        assert_eq!(*recorder.acquired.lock().unwrap(), 1);
        assert_eq!(*recorder.statuses.lock().unwrap(), vec![429]);
    }
}
