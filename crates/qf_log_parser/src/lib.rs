//! Parses Warframe trade dialogs out of EE.log (spec §5.8, amendment E2).
//!
//! Warframe writes the accept dialog in arbitrary chunks, so nothing here assumes line boundaries:
//! the [`scanner::Scanner`] accumulates bytes and acts only on complete markers.

pub mod scanner;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use scanner::Scanner;

pub const START: &str = "Dialog::CreateOkCancel(description=Are you sure you want to accept this trade? You are offering:";
pub const END: &str = ", title= leftItem=/Menu/Confirm_Item_Ok, rightItem=/Menu/Confirm_Item_Cancel)";
pub const SUCCESS: &str = "description=The trade was successful!";
pub const FAILED: &str = "description=The trade failed.";
pub const CANCELLED: &str = "description=The trade was cancelled";
pub const ACCEPT_FAILED: &str = "OnTradeAccepted failed";
const RECEIVE_FROM: &str = "and will receive from ";
const THE_FOLLOWING: &str = " the following:";
const PLATINUM_PREFIX: &str = "Platinum x ";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawItem {
    pub name: String,
    pub quantity: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawTrade {
    pub player_name: String,
    /// The EE.log timestamp of the dialog's start line, e.g. `422.424`.
    pub ee_timestamp: String,
    pub offered: Vec<RawItem>,
    pub received: Vec<RawItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TradeEvent {
    pub event_id: String,
    pub trade: RawTrade,
}

/// Warframe appends rank icons and name badges from the private-use area.
fn is_private_use(c: char) -> bool {
    ('\u{e000}'..='\u{f8ff}').contains(&c)
}

/// Removes trailing private-use characters and returns how many there were.
fn strip_glyphs(text: &str) -> (&str, usize) {
    let trimmed = text.trim_end_matches(is_private_use);
    (trimmed, text[trimmed.len()..].chars().count())
}

/// One dialog piece: `Platinum x 30`, `Adaptation (RARE RANK 10)`, `Arcane Energize <5 glyphs>`, `Wolf Sledge Handle`.
pub fn parse_line(piece: &str) -> RawItem {
    let piece = piece.trim();
    if let Some(amount) = piece.strip_prefix(PLATINUM_PREFIX) {
        return RawItem { name: "Platinum".into(), quantity: amount.trim().parse().unwrap_or(1), rank: None };
    }
    let (without_glyphs, glyphs) = strip_glyphs(piece);
    let without_glyphs = without_glyphs.trim();
    if glyphs > 0 {
        return RawItem { name: without_glyphs.to_string(), quantity: 1, rank: Some(glyphs as i64) };
    }
    if let Some(open) = without_glyphs.rfind(" (") {
        if let Some(inner) = without_glyphs[open + 2..].strip_suffix(')') {
            if let Some((_, rank)) = inner.rsplit_once(" RANK ") {
                if let Ok(rank) = rank.trim().parse::<i64>() {
                    return RawItem { name: without_glyphs[..open].trim().to_string(), quantity: 1, rank: Some(rank) };
                }
            }
        }
    }
    RawItem { name: without_glyphs.to_string(), quantity: 1, rank: None }
}

fn parse_pieces(text: &str) -> Vec<RawItem> {
    let mut items: Vec<RawItem> = Vec::new();
    for piece in text.split(['\r', '\n']).map(str::trim).filter(|p| !p.is_empty()) {
        let item = parse_line(piece);
        match items.iter_mut().find(|i| i.name == item.name && i.rank == item.rank) {
            Some(existing) => existing.quantity += item.quantity,
            None => items.push(item),
        }
    }
    items
}

/// Splits the text between [`START`] and [`END`] into the offered and received items.
pub fn parse_dialog(ee_timestamp: &str, dialog: &str) -> RawTrade {
    let (offered_text, rest) = dialog.split_once(RECEIVE_FROM).unwrap_or((dialog, ""));
    let (player, received_text) = rest.split_once(THE_FOLLOWING).unwrap_or((rest, ""));
    let (player, _) = strip_glyphs(player.trim());
    RawTrade {
        player_name: player.trim().to_string(),
        ee_timestamp: ee_timestamp.to_string(),
        offered: parse_pieces(offered_text),
        received: parse_pieces(received_text),
    }
}

/// SHA-256 hex of `<ee_timestamp>\n<dialog>` (amendment E2).
pub fn event_id(ee_timestamp: &str, dialog: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(ee_timestamp.as_bytes());
    hasher.update(b"\n");
    hasher.update(dialog.as_bytes());
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Scans a whole file. Same result as feeding it to a [`Scanner`] in any chunking.
pub fn scan_all(bytes: &[u8]) -> Vec<TradeEvent> {
    Scanner::new().feed(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platinum_lines_carry_the_amount() {
        assert_eq!(parse_line("Platinum x 30"), RawItem { name: "Platinum".into(), quantity: 30, rank: None });
        assert_eq!(parse_line("Platinum x 70\r").quantity, 70);
    }

    #[test]
    fn mod_ranks_come_from_the_suffix_whatever_the_word() {
        assert_eq!(parse_line("Adaptation (RARE RANK 10)"), RawItem { name: "Adaptation".into(), quantity: 1, rank: Some(10) });
        assert_eq!(parse_line("Galvanized Shot (GALVANIZED RANK 10)").name, "Galvanized Shot");
        assert_eq!(parse_line("Archon Vitality (KAHL RANK 10)").rank, Some(10));
        assert_eq!(parse_line("Pistol Gambit (COMMON RANK 0)").rank, Some(0));
    }

    #[test]
    fn arcane_glyph_count_is_the_rank() {
        let five = "Arcane Nullifier \u{e0b9}\u{e0b9}\u{e0b9}\u{e0b9}\u{e0b9}";
        assert_eq!(parse_line(five), RawItem { name: "Arcane Nullifier".into(), quantity: 1, rank: Some(5) });
        let three = "Exodia Contagion \u{e0dc}\u{e0dc}\u{e0dc}";
        assert_eq!(parse_line(three).rank, Some(3));
    }

    #[test]
    fn plain_names_and_names_with_parentheses_but_no_rank_are_kept() {
        assert_eq!(parse_line("Wolf Sledge Handle"), RawItem { name: "Wolf Sledge Handle".into(), quantity: 1, rank: None });
        assert_eq!(parse_line("Mortus Lungfish (L)"), RawItem { name: "Mortus Lungfish (L)".into(), quantity: 1, rank: None });
    }

    #[test]
    fn dialog_splits_sides_folds_duplicates_and_cleans_the_player() {
        let dialog = "\r\nPlatinum x 300\r\n\r\nand will receive from CloudStepKing\u{e000} the following:\
                      \r\nGalvanized Shot (GALVANIZED RANK 10)\r\nGalvanized Shot (GALVANIZED RANK 10)\r\nParry (COMMON RANK 0)";
        let trade = parse_dialog("5380.793", dialog);
        assert_eq!(trade.player_name, "CloudStepKing");
        assert_eq!(trade.ee_timestamp, "5380.793");
        assert_eq!(trade.offered, vec![RawItem { name: "Platinum".into(), quantity: 300, rank: None }]);
        assert_eq!(
            trade.received,
            vec![
                RawItem { name: "Galvanized Shot".into(), quantity: 2, rank: Some(10) },
                RawItem { name: "Parry".into(), quantity: 1, rank: Some(0) },
            ]
        );
    }

    #[test]
    fn event_ids_are_stable_hex_and_depend_on_the_timestamp() {
        let a = event_id("1.000", "x");
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(a, event_id("1.000", "x"));
        assert_ne!(a, event_id("2.000", "x"));
    }
}
