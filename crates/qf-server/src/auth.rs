use std::{
    collections::{HashMap, VecDeque},
    sync::Mutex,
    time::{Duration, Instant},
};

use axum::http::{header, HeaderMap};

pub const COOKIE_NAME: &str = "qf_session";
const COOKIE_MAX_AGE_SECS: u64 = 30 * 24 * 60 * 60;

pub struct Sessions {
    inner: Mutex<HashMap<String, Instant>>,
    ttl: Duration,
}

impl Sessions {
    pub fn new(ttl: Duration) -> Self {
        Self { inner: Mutex::new(HashMap::new()), ttl }
    }

    pub fn create(&self) -> String {
        let mut bytes = [0u8; 32];
        getrandom::getrandom(&mut bytes).expect("OS random number generator unavailable");
        let token = hex::encode(bytes);
        self.inner.lock().unwrap().insert(token.clone(), Instant::now() + self.ttl);
        token
    }

    pub fn is_valid(&self, token: &str) -> bool {
        let mut sessions = self.inner.lock().unwrap();
        let now = Instant::now();
        sessions.retain(|_, expires| *expires > now);
        sessions.contains_key(token)
    }

    pub fn remove(&self, token: &str) {
        self.inner.lock().unwrap().remove(token);
    }
}

/// Allows at most `max` login attempts per `window`, across all clients.
pub struct LoginLimiter {
    attempts: Mutex<VecDeque<Instant>>,
    max: usize,
    window: Duration,
}

impl LoginLimiter {
    pub fn new(max: usize, window: Duration) -> Self {
        Self { attempts: Mutex::new(VecDeque::new()), max, window }
    }

    pub fn try_acquire(&self) -> bool {
        let mut attempts = self.attempts.lock().unwrap();
        let now = Instant::now();
        while attempts.front().is_some_and(|t| now.duration_since(*t) > self.window) {
            attempts.pop_front();
        }
        if attempts.len() >= self.max {
            return false;
        }
        attempts.push_back(now);
        true
    }
}

pub fn session_cookie(token: &str) -> String {
    format!("{COOKIE_NAME}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age={COOKIE_MAX_AGE_SECS}")
}

pub fn cleared_cookie() -> String {
    format!("{COOKIE_NAME}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0")
}

pub fn token_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == COOKIE_NAME)
        .map(|(_, value)| value.to_string())
}
