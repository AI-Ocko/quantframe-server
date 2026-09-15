use qf_log_parser::{scan_all, RawItem, Scanner, TradeEvent};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn item(name: &str, quantity: i64, rank: Option<i64>) -> RawItem {
    RawItem { name: name.into(), quantity, rank }
}

fn only(events: Vec<TradeEvent>) -> TradeEvent {
    assert_eq!(events.len(), 1, "expected exactly one trade");
    events.into_iter().next().unwrap()
}

#[test]
fn purchase_of_set_parts() {
    let event = only(scan_all(&fixture("purchase_set_parts.log")));
    assert_eq!(event.trade.player_name, "PlayerA");
    assert_eq!(event.trade.offered, vec![item("Platinum", 30, None)]);
    assert_eq!(
        event.trade.received,
        vec![
            item("Wolf Sledge Blueprint", 1, None),
            item("Wolf Sledge Motor", 1, None),
            item("Wolf Sledge Head", 1, None),
            item("Wolf Sledge Handle", 1, None),
        ]
    );
}

#[test]
fn sale_of_an_arcane_at_rank_five() {
    let event = only(scan_all(&fixture("sale_arcane.log")));
    assert_eq!(event.trade.offered, vec![item("Arcane Nullifier", 1, Some(5))]);
    assert_eq!(event.trade.received, vec![item("Platinum", 70, None)]);
    assert_eq!(event.trade.ee_timestamp, "1170.388");
}

#[test]
fn a_mod_name_written_in_two_chunks_is_one_item() {
    let event = only(scan_all(&fixture("wrapped_mod_name.log")));
    assert_eq!(event.trade.received, vec![item("Galvanized Scope", 1, Some(10))]);
}

#[test]
fn an_end_marker_written_in_two_chunks_still_ends_the_dialog() {
    let event = only(scan_all(&fixture("split_end_marker.log")));
    assert_eq!(event.trade.offered, vec![item("Arcane Ice Storm", 1, Some(5))]);
    assert_eq!(event.trade.received, vec![item("Platinum", 70, None)]);
}

#[test]
fn repeated_lines_fold_into_quantity() {
    let event = only(scan_all(&fixture("quantity_repeated_lines.log")));
    assert_eq!(event.trade.received, vec![item("Galvanized Shot", 6, Some(10))]);
}

#[test]
fn extras_beside_platinum_are_kept_as_raw_items() {
    let event = only(scan_all(&fixture("extras_with_platinum.log")));
    assert_eq!(event.trade.offered.len(), 4);
    assert_eq!(
        event.trade.received,
        vec![item("Platinum", 43, None), item("Parry", 1, Some(0)), item("Pistol Gambit", 3, Some(0))]
    );
}

#[test]
fn failed_cancelled_and_accept_failed_produce_nothing() {
    for name in ["result_failed.log", "result_cancelled.log", "result_accept_failed.log"] {
        assert!(scan_all(&fixture(name)).is_empty(), "{name}");
    }
}

#[test]
fn every_fixture_is_chunk_size_invariant() {
    for name in [
        "purchase_set_parts.log",
        "sale_arcane.log",
        "wrapped_mod_name.log",
        "split_end_marker.log",
        "quantity_repeated_lines.log",
        "extras_with_platinum.log",
        "result_failed.log",
        "result_cancelled.log",
        "result_accept_failed.log",
    ] {
        let bytes = fixture(name);
        let whole = scan_all(&bytes);
        for size in [1usize, 7, 64] {
            let mut scanner = Scanner::new();
            let mut events = Vec::new();
            for chunk in bytes.chunks(size) {
                events.extend(scanner.feed(chunk));
            }
            assert_eq!(events, whole, "{name} at chunk size {size}");
        }
    }
}

#[test]
fn event_ids_differ_between_fixtures_and_repeat_for_the_same_bytes() {
    let a = only(scan_all(&fixture("purchase_set_parts.log"))).event_id;
    let b = only(scan_all(&fixture("sale_arcane.log"))).event_id;
    assert_ne!(a, b);
    assert_eq!(a, only(scan_all(&fixture("purchase_set_parts.log"))).event_id);
}
