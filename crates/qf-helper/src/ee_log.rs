use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

use qf_log_parser::{Scanner, TradeEvent};

pub const POLL_EVERY: Duration = Duration::from_secs(1);

/// Follows EE.log from its current end (amendment E3). Earlier trades are never replayed.
pub struct Tail {
    path: PathBuf,
    offset: u64,
    scanner: Scanner,
    warned_missing: bool,
}

impl Tail {
    pub fn start_at_end(path: &Path) -> Self {
        let offset = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        Self { path: path.to_path_buf(), offset, scanner: Scanner::new(), warned_missing: false }
    }

    pub fn offset(&self) -> u64 {
        self.offset
    }

    /// Reads whatever was appended since the last poll and returns the trades completed in it.
    /// A file shorter than the last offset means a new game session: reading restarts from 0.
    pub fn poll(&mut self) -> Vec<TradeEvent> {
        let len = match std::fs::metadata(&self.path) {
            Ok(meta) => meta.len(),
            Err(_) => {
                if !self.warned_missing {
                    eprintln!("EE.log not found at {}; waiting for Warframe to create it", self.path.display());
                    self.warned_missing = true;
                }
                return Vec::new();
            }
        };
        self.warned_missing = false;
        if len < self.offset {
            println!("EE.log was truncated (new game session); reading it from the start");
            self.offset = 0;
            self.scanner.reset();
        }
        if len == self.offset {
            return Vec::new();
        }
        let mut file = match File::open(&self.path) {
            Ok(file) => file,
            Err(e) => {
                eprintln!("cannot open {}: {e}", self.path.display());
                return Vec::new();
            }
        };
        if let Err(e) = file.seek(SeekFrom::Start(self.offset)) {
            eprintln!("cannot seek {}: {e}", self.path.display());
            return Vec::new();
        }
        let mut bytes = Vec::with_capacity((len - self.offset) as usize);
        if let Err(e) = file.take(len - self.offset).read_to_end(&mut bytes) {
            eprintln!("cannot read {}: {e}", self.path.display());
            return Vec::new();
        }
        self.offset += bytes.len() as u64;
        self.scanner.feed(&bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const DIALOG: &str = "422.424 Script [Info]: Dialog.lua: Dialog::CreateOkCancel(description=Are you sure you want to accept this trade? You are offering:\n\
\rPlatinum x 30\r\n\r\nand will receive from PlayerA\u{e000} the following:\n\
\rWolf Sledge Handle, title= leftItem=/Menu/Confirm_Item_Ok, rightItem=/Menu/Confirm_Item_Cancel)\n";
    const OK: &str = "427.411 Script [Info]: Dialog.lua: Dialog::CreateOk(description=The trade was successful!, title= leftItem=/Menu/Confirm_Item_Ok)\n";

    fn append(path: &Path, text: &str) {
        let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap();
        file.write_all(text.as_bytes()).unwrap();
    }

    #[test]
    fn starts_at_the_end_and_reports_trades_completed_across_polls() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("EE.log");
        append(&path, "0.001 Sys [Info]: old session\n");
        append(&path, DIALOG);
        append(&path, OK);
        let mut tail = Tail::start_at_end(&path);
        assert_eq!(tail.offset(), std::fs::metadata(&path).unwrap().len());
        assert!(tail.poll().is_empty(), "existing trades are never replayed");

        append(&path, DIALOG);
        assert!(tail.poll().is_empty(), "no result yet");
        append(&path, OK);
        let events = tail.poll();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].trade.received[0].name, "Wolf Sledge Handle");
        assert!(tail.poll().is_empty());
    }

    #[test]
    fn a_shorter_file_is_a_new_session_and_is_read_from_the_start() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("EE.log");
        append(&path, &"x".repeat(5000));
        let mut tail = Tail::start_at_end(&path);
        std::fs::write(&path, format!("{DIALOG}{OK}")).unwrap();
        assert_eq!(tail.poll().len(), 1);
        assert_eq!(tail.offset(), (DIALOG.len() + OK.len()) as u64);
    }

    #[test]
    fn a_missing_file_yields_nothing_until_it_appears() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("EE.log");
        let mut tail = Tail::start_at_end(&path);
        assert!(tail.poll().is_empty());
        std::fs::write(&path, format!("{DIALOG}{OK}")).unwrap();
        assert_eq!(tail.poll().len(), 1, "a file that appears is read from offset 0");
    }
}
