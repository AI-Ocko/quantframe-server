use std::io::Write;
use std::path::{Path, PathBuf};

use qf_log_parser::RawTrade;
use serde::{Deserialize, Serialize};

/// One line of the queue file and the body of `POST /helper/trade` (amendment E4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueuedEvent {
    pub event_id: String,
    /// RFC 3339 UTC, when the helper saw the success line.
    pub detected_at: String,
    pub trade: RawTrade,
}

/// `$XDG_STATE_HOME/qf-helper/trade-queue.jsonl`, or `~/.local/state/qf-helper/trade-queue.jsonl`.
pub fn default_queue_path(xdg_state_home: Option<&str>, home: &Path) -> PathBuf {
    let base = match xdg_state_home {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => home.join(".local").join("state"),
    };
    base.join("qf-helper").join("trade-queue.jsonl")
}

/// Append-only JSONL, replayed oldest first (amendment E3).
pub struct Queue {
    path: PathBuf,
}

impl Queue {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn lines(&self) -> Result<Vec<String>, String> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => Ok(text.lines().filter(|l| !l.trim().is_empty()).map(str::to_string).collect()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(format!("cannot read {}: {e}", self.path.display())),
        }
    }

    pub fn push(&self, event: &QueuedEvent) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        let line = serde_json::to_string(event).map_err(|e| e.to_string())?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| format!("cannot open {}: {e}", self.path.display()))?;
        writeln!(file, "{line}").map_err(|e| format!("cannot write {}: {e}", self.path.display()))?;
        // A detected trade must be on disk before the tail moves on (spec §19 H4).
        file.sync_all().map_err(|e| format!("cannot sync {}: {e}", self.path.display()))
    }

    /// The oldest event. A corrupt line is dropped with a message so the queue can't wedge.
    pub fn peek(&self) -> Result<Option<QueuedEvent>, String> {
        loop {
            let Some(first) = self.lines()?.into_iter().next() else { return Ok(None) };
            match serde_json::from_str::<QueuedEvent>(&first) {
                Ok(event) => return Ok(Some(event)),
                Err(e) => {
                    eprintln!("dropping corrupt queue line: {e}");
                    self.pop()?;
                }
            }
        }
    }

    pub fn pop(&self) -> Result<(), String> {
        let mut lines = self.lines()?;
        if lines.is_empty() {
            return Ok(());
        }
        lines.remove(0);
        let mut text = lines.join("\n");
        if !text.is_empty() {
            text.push('\n');
        }
        // Replaced atomically: a crash between truncating and writing would lose every queued event.
        let mut temp = self.path.clone().into_os_string();
        temp.push(".tmp");
        let temp = PathBuf::from(temp);
        std::fs::write(&temp, text).map_err(|e| format!("cannot write {}: {e}", temp.display()))?;
        std::fs::rename(&temp, &self.path).map_err(|e| format!("cannot replace {}: {e}", self.path.display()))
    }

    pub fn len(&self) -> usize {
        self.lines().map(|l| l.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qf_log_parser::RawItem;

    fn event(id: &str) -> QueuedEvent {
        QueuedEvent {
            event_id: id.into(),
            detected_at: "2026-09-15T10:00:00Z".into(),
            trade: RawTrade {
                player_name: "PlayerA".into(),
                ee_timestamp: "1.000".into(),
                offered: vec![RawItem { name: "Platinum".into(), quantity: 30, rank: None }],
                received: vec![RawItem { name: "Wolf Sledge Handle".into(), quantity: 1, rank: None }],
            },
        }
    }

    #[test]
    fn events_replay_oldest_first_and_pop_removes_the_head() {
        let dir = tempfile::tempdir().unwrap();
        let queue = Queue::new(dir.path().join("state/qf-helper/trade-queue.jsonl"));
        assert!(queue.peek().unwrap().is_none());
        queue.push(&event("a")).unwrap();
        queue.push(&event("b")).unwrap();
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.peek().unwrap().unwrap().event_id, "a");
        queue.pop().unwrap();
        assert_eq!(queue.peek().unwrap().unwrap().event_id, "b");
        queue.pop().unwrap();
        assert!(queue.is_empty());
        queue.pop().unwrap();
    }

    #[test]
    fn pop_keeps_the_rest_of_the_queue_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trade-queue.jsonl");
        let queue = Queue::new(path.clone());
        for id in ["a", "b", "c"] {
            queue.push(&event(id)).unwrap();
        }
        queue.pop().unwrap();
        let left: Vec<String> = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str::<QueuedEvent>(l).unwrap().event_id)
            .collect();
        assert_eq!(left, ["b", "c"], "pop removes only the head");
        assert!(!dir.path().join("trade-queue.jsonl.tmp").exists(), "the temp file is renamed away");
    }

    #[test]
    fn corrupt_lines_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trade-queue.jsonl");
        std::fs::write(&path, "not json\n").unwrap();
        let queue = Queue::new(path);
        queue.push(&event("c")).unwrap();
        assert_eq!(queue.peek().unwrap().unwrap().event_id, "c");
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn queue_path_prefers_xdg_state_home() {
        let home = Path::new("/home/player");
        assert_eq!(default_queue_path(Some("/st"), home), PathBuf::from("/st/qf-helper/trade-queue.jsonl"));
        assert_eq!(default_queue_path(None, home), PathBuf::from("/home/player/.local/state/qf-helper/trade-queue.jsonl"));
    }
}
