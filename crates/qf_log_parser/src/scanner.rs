use crate::{event_id, parse_dialog, RawTrade, TradeEvent, ACCEPT_FAILED, CANCELLED, END, FAILED, START, SUCCESS};

/// Longest tail worth keeping while waiting for a marker: a dialog line plus slack.
const KEEP_TAIL: usize = 64 * 1024;

struct Pending {
    ee_timestamp: String,
    dialog: String,
}

/// A chunk-safe state machine: feed it EE.log bytes in any sizes and it emits successful trades.
#[derive(Default)]
pub struct Scanner {
    buf: Vec<u8>,
    pending: Option<Pending>,
}

fn find(haystack: &[u8], needle: &str, from: usize) -> Option<usize> {
    let needle = needle.as_bytes();
    if from > haystack.len() || needle.is_empty() {
        return None;
    }
    haystack[from..].windows(needle.len()).position(|w| w == needle).map(|p| p + from)
}

/// Start of the line that contains `at`.
fn line_start(buf: &[u8], at: usize) -> usize {
    buf[..at].iter().rposition(|b| *b == b'\n').map_or(0, |p| p + 1)
}

/// `422.424 Script [Info]: ...` -> `422.424`
fn timestamp_of(line: &[u8]) -> String {
    let text = String::from_utf8_lossy(line);
    text.split_whitespace().next().unwrap_or("").to_string()
}

impl Scanner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Forgets everything, e.g. after EE.log was truncated.
    pub fn reset(&mut self) {
        self.buf.clear();
        self.pending = None;
    }

    pub fn feed(&mut self, chunk: &[u8]) -> Vec<TradeEvent> {
        self.buf.extend_from_slice(chunk);
        let mut events = Vec::new();
        loop {
            if self.pending.is_none() {
                let Some(start) = find(&self.buf, START, 0) else {
                    self.keep_tail_from_line_start();
                    break;
                };
                let dialog_from = start + START.len();
                let Some(end) = find(&self.buf, END, dialog_from) else {
                    // The dialog is still being written: keep from its line start and wait.
                    let keep = line_start(&self.buf, start);
                    self.buf.drain(..keep);
                    break;
                };
                let ee_timestamp = timestamp_of(&self.buf[line_start(&self.buf, start)..start]);
                let dialog = String::from_utf8_lossy(&self.buf[dialog_from..end]).into_owned();
                self.pending = Some(Pending { ee_timestamp, dialog });
                self.buf.drain(..end + END.len());
                continue;
            }

            // Waiting for the result of the pending dialog.
            let candidates = [
                (find(&self.buf, SUCCESS, 0), "success"),
                (find(&self.buf, FAILED, 0), "discard"),
                (find(&self.buf, CANCELLED, 0), "discard"),
                (find(&self.buf, ACCEPT_FAILED, 0), "discard"),
                (find(&self.buf, START, 0), "restart"),
            ];
            let Some((pos, kind, len)) = candidates
                .iter()
                .zip([SUCCESS.len(), FAILED.len(), CANCELLED.len(), ACCEPT_FAILED.len(), START.len()])
                .filter_map(|((pos, kind), len)| pos.map(|p| (p, *kind, len)))
                .min_by_key(|(p, _, _)| *p)
            else {
                self.keep_tail_from_line_start();
                break;
            };
            let pending = self.pending.take().expect("pending checked above");
            match kind {
                "success" => {
                    let trade: RawTrade = parse_dialog(&pending.ee_timestamp, &pending.dialog);
                    events.push(TradeEvent { event_id: event_id(&pending.ee_timestamp, &pending.dialog), trade });
                    self.buf.drain(..pos + len);
                }
                "discard" => {
                    self.buf.drain(..pos + len);
                }
                _ => {
                    // A new dialog before any result: the loop picks it up with `pending == None`.
                }
            }
        }
        events
    }

    /// Keeps the current (possibly partial) last line so a marker split across chunks is still found.
    fn keep_tail_from_line_start(&mut self) {
        let keep = line_start(&self.buf, self.buf.len());
        let keep = keep.min(self.buf.len());
        let keep = if self.buf.len() - keep > KEEP_TAIL { self.buf.len() - KEEP_TAIL } else { keep };
        self.buf.drain(..keep);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PURCHASE: &str = "422.424 Sys [Info]: Created /Lotus/Interface/Dialog.swf\n\
422.424 Script [Info]: Dialog.lua: Dialog::CreateOkCancel(description=Are you sure you want to accept this trade? You are offering:\n\
\rPlatinum x 30\r\n\
\r\n\
and will receive from PlayerA\u{e000} the following:\n\
\rWolf Sledge Blueprint\n\
\rWolf Sledge Motor\n\
\rWolf Sledge Head\n\
\rWolf Sledge Handle, title= leftItem=/Menu/Confirm_Item_Ok, rightItem=/Menu/Confirm_Item_Cancel)\n\
424.897 Net [Info]: Updating session (params changed)\n\
426.113 Script [Info]: Dialog.lua: Dialog::SendResult(4)\n\
427.411 Sys [Info]: Created /Lotus/Interface/Dialog.swf\n\
427.411 Script [Info]: Dialog.lua: Dialog::CreateOk(description=The trade was successful!, title= leftItem=/Menu/Confirm_Item_Ok)\n";

    #[test]
    fn a_successful_purchase_is_one_event() {
        let events = Scanner::new().feed(PURCHASE.as_bytes());
        assert_eq!(events.len(), 1);
        let trade = &events[0].trade;
        assert_eq!(trade.ee_timestamp, "422.424");
        assert_eq!(trade.player_name, "PlayerA");
        assert_eq!(trade.offered[0].quantity, 30);
        assert_eq!(trade.received.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["Wolf Sledge Blueprint", "Wolf Sledge Motor", "Wolf Sledge Head", "Wolf Sledge Handle"]);
        assert_eq!(events[0].event_id.len(), 64);
    }

    #[test]
    fn byte_by_byte_feeding_gives_the_same_event() {
        let whole = Scanner::new().feed(PURCHASE.as_bytes());
        let mut scanner = Scanner::new();
        let mut events = Vec::new();
        for byte in PURCHASE.as_bytes() {
            events.extend(scanner.feed(std::slice::from_ref(byte)));
        }
        assert_eq!(events, whole);
    }

    #[test]
    fn failed_cancelled_and_accept_failed_results_are_dropped() {
        for result in [
            "1.000 Script [Info]: Dialog.lua: Dialog::CreateOk(description=The trade failed., title= leftItem=/Menu/Confirm_Item_Ok)\n",
            "1.000 Script [Info]: Dialog.lua: Dialog::CreateOk(description=The trade was cancelled, title= leftItem=/Menu/Confirm_Item_Ok)\n",
            "849.134 Sys [Info]: OnTradeAccepted failed: -13\n",
        ] {
            let text = PURCHASE.replace(
                "427.411 Script [Info]: Dialog.lua: Dialog::CreateOk(description=The trade was successful!, title= leftItem=/Menu/Confirm_Item_Ok)\n",
                result,
            );
            let mut scanner = Scanner::new();
            assert!(scanner.feed(text.as_bytes()).is_empty(), "{result}");
            // A later, unrelated success must not resurrect the dropped dialog.
            assert!(scanner.feed(b"2.000 Script [Info]: Dialog.lua: Dialog::CreateOk(description=The trade was successful!, title=)\n").is_empty());
        }
    }

    #[test]
    fn a_new_dialog_before_a_result_replaces_the_pending_one() {
        let first_half = PURCHASE.split("424.897").next().unwrap();
        let text = format!("{first_half}{PURCHASE}");
        let events = Scanner::new().feed(text.as_bytes());
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn a_success_without_a_dialog_is_ignored_and_reset_clears_state() {
        let mut scanner = Scanner::new();
        assert!(scanner.feed(b"1.0 Script [Info]: Dialog.lua: Dialog::CreateOk(description=The trade was successful!, title=)\n").is_empty());
        let first_half = PURCHASE.split("424.897").next().unwrap();
        scanner.feed(first_half.as_bytes());
        scanner.reset();
        assert!(scanner.feed(b"9.0 Script [Info]: Dialog.lua: Dialog::CreateOk(description=The trade was successful!, title=)\n").is_empty());
    }
}
