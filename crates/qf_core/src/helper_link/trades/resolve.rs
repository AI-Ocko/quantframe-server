//! In-game names to WFM items (amendments E1, E6, E7).

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;
use utils::{get_location, warning, Error, LoggerOptions, SubType};

use super::{Direction, RawItem, RawTrade, ResolvedItem, Resolution};
use crate::cache::types::CacheTradableItem;

pub const OVERRIDES_FILE: &str = "overrides.toml";
pub const PLATINUM: &str = "Platinum";

fn is_private_use(c: char) -> bool {
    ('\u{e000}'..='\u{f8ff}').contains(&c)
}

/// Trim, drop trailing private-use glyphs, collapse whitespace, lowercase.
pub fn normalise(name: &str) -> String {
    name.trim().trim_end_matches(is_private_use).split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

#[derive(Debug, Default, Deserialize)]
struct OverridesFile {
    #[serde(default)]
    names: HashMap<String, String>,
}

/// `[names]` table of in-game display name to WFM slug.
#[derive(Debug, Default, Clone)]
pub struct Overrides {
    names: HashMap<String, String>,
}

impl Overrides {
    pub fn parse(text: &str) -> Result<Self, Error> {
        let file: OverridesFile = toml::from_str(text)
            .map_err(|e| Error::new("HelperLink:Overrides", format!("Invalid overrides.toml: {e}"), get_location!()))?;
        Ok(Self { names: file.names.into_iter().map(|(name, slug)| (normalise(&name), slug.trim().to_string())).collect() })
    }

    /// A missing file is empty; an invalid one is logged and treated as empty.
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text).unwrap_or_else(|e| {
                warning("HelperLink:Overrides", format!("{} ignored: {}", path.display(), e.message), &LoggerOptions::default());
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn slug_for(&self, name: &str) -> Option<&str> {
        self.names.get(&normalise(name)).map(String::as_str)
    }
}

/// The tradable item list keyed three ways.
pub struct ItemIndex {
    by_name: HashMap<String, CacheTradableItem>,
    by_slug: HashMap<String, CacheTradableItem>,
    by_id: HashMap<String, CacheTradableItem>,
}

impl ItemIndex {
    pub fn from_items(items: Vec<CacheTradableItem>) -> Self {
        let mut index = Self { by_name: HashMap::new(), by_slug: HashMap::new(), by_id: HashMap::new() };
        for item in items {
            index.by_name.entry(normalise(&item.name)).or_insert_with(|| item.clone());
            index.by_id.insert(item.wfm_id.clone(), item.clone());
            index.by_slug.insert(item.wfm_url.clone(), item);
        }
        index
    }

    pub fn by_name(&self, name: &str) -> Option<&CacheTradableItem> {
        self.by_name.get(&normalise(name))
    }

    pub fn by_slug(&self, slug: &str) -> Option<&CacheTradableItem> {
        self.by_slug.get(slug)
    }

    pub fn by_id(&self, id: &str) -> Option<&CacheTradableItem> {
        self.by_id.get(id)
    }

    pub fn set_roots(&self) -> impl Iterator<Item = &CacheTradableItem> {
        self.by_slug.values().filter(|item| item.tags.iter().any(|t| t == "set"))
    }
}

#[derive(Debug)]
pub struct Classified {
    pub direction: Direction,
    pub platinum: i64,
    pub goods: Vec<RawItem>,
    pub extras: Vec<RawItem>,
}

fn platinum_of(items: &[RawItem]) -> i64 {
    items.iter().filter(|i| i.name == PLATINUM).map(|i| i.quantity).sum()
}

fn without_platinum(items: &[RawItem]) -> Vec<RawItem> {
    items.iter().filter(|i| i.name != PLATINUM).cloned().collect()
}

/// Amendment E6. The error is the review reason.
pub fn classify(trade: &RawTrade) -> Result<Classified, String> {
    let offered = platinum_of(&trade.offered);
    let received = platinum_of(&trade.received);
    match (offered > 0, received > 0) {
        (true, false) => Ok(Classified {
            direction: Direction::Purchase,
            platinum: offered,
            goods: without_platinum(&trade.received),
            extras: without_platinum(&trade.offered),
        }),
        (false, true) => Ok(Classified {
            direction: Direction::Sale,
            platinum: received,
            goods: without_platinum(&trade.offered),
            extras: without_platinum(&trade.received),
        }),
        _ => Err("no_platinum_side".to_string()),
    }
}

fn resolved(raw: &RawItem, item: &CacheTradableItem, matched_by: &str) -> ResolvedItem {
    let sub_type = raw.rank.map(|rank| {
        let capped = item.sub_type.as_ref().and_then(|s| s.max_rank).map_or(rank, |max| rank.min(max));
        SubType::rank(capped)
    });
    ResolvedItem {
        name: raw.name.clone(),
        slug: item.wfm_url.clone(),
        wfm_id: item.wfm_id.clone(),
        item_name: item.name.clone(),
        sub_type,
        quantity: raw.quantity,
        price: 0,
        matched_by: matched_by.into(),
    }
}

/// English name first, then `overrides.toml`.
pub fn resolve_item(raw: &RawItem, index: &ItemIndex, overrides: &Overrides) -> Option<ResolvedItem> {
    if let Some(item) = index.by_name(&raw.name) {
        return Some(resolved(raw, item, "name"));
    }
    let slug = overrides.slug_for(&raw.name)?;
    index.by_slug(slug).map(|item| resolved(raw, item, "override"))
}

pub struct Resolved {
    pub resolution: Resolution,
    /// Raw names that resolved to nothing; empty means every goods item resolved.
    pub unresolved: Vec<String>,
}

pub fn resolve_trade(trade: &RawTrade, index: &ItemIndex, overrides: &Overrides) -> Result<Resolved, String> {
    let classified = classify(trade)?;
    let mut items = Vec::new();
    let mut unresolved = Vec::new();
    for raw in &classified.goods {
        match resolve_item(raw, index, overrides) {
            Some(item) => items.push(item),
            None => unresolved.push(raw.name.clone()),
        }
    }
    Ok(Resolved {
        resolution: Resolution {
            direction: Some(classified.direction),
            platinum: classified.platinum,
            items,
            extras: classified.extras,
        },
        unresolved,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::cache::types::SubType as CacheSubType;

    pub(crate) fn item(name: &str, slug: &str, max_rank: Option<i64>, tags: &[&str]) -> CacheTradableItem {
        CacheTradableItem {
            name: name.into(),
            unique_name: String::new(),
            wfm_id: format!("id_{slug}"),
            wfm_url: slug.into(),
            trade_tax: 0,
            mr_requirement: 0,
            tags: tags.iter().map(|t| t.to_string()).collect(),
            icon: String::new(),
            bulk_tradable: false,
            sub_type: max_rank.map(|max| CacheSubType { max_rank: Some(max), variants: None, amber_stars: None, cyan_stars: None }),
            variant_to_unique_name: HashMap::new(),
        }
    }

    /// The small item list every trades test shares (Task 6 reuses it).
    pub(crate) fn items() -> Vec<CacheTradableItem> {
        vec![
            item("Arcane Nullifier", "arcane_nullifier", Some(5), &["arcane_enhancement"]),
            item("Adaptation", "adaptation", Some(10), &["mod"]),
            item("Wolf Sledge Set", "wolf_sledge_set", None, &["set", "weapon"]),
            item("Wolf Sledge Blueprint", "wolf_sledge_blueprint", None, &["component"]),
            item("Wolf Sledge Motor", "wolf_sledge_motor", None, &["component"]),
            item("Wolf Sledge Head", "wolf_sledge_head", None, &["component"]),
            item("Wolf Sledge Handle", "wolf_sledge_handle", None, &["component"]),
            item("Mesa Prime Set", "mesa_prime_set", None, &["set", "prime"]),
            item("Primed Firestorm", "primed_firestorm", Some(10), &["mod"]),
        ]
    }

    pub(crate) fn index() -> ItemIndex {
        ItemIndex::from_items(items())
    }

    fn raw(name: &str, quantity: i64, rank: Option<i64>) -> RawItem {
        RawItem { name: name.into(), quantity, rank }
    }

    #[test]
    fn normalise_trims_glyphs_whitespace_and_case() {
        assert_eq!(normalise("  Arcane  Energize \u{e0b9}\u{e0b9} "), "arcane energize");
        assert_eq!(normalise("Wolf Sledge Handle"), "wolf sledge handle");
    }

    #[test]
    fn overrides_parse_load_and_lookup() {
        let overrides = Overrides::parse("[names]\n\"Primed Fir\" = \"primed_firestorm\"\n").unwrap();
        assert_eq!(overrides.slug_for("primed fir \u{e000}"), Some("primed_firestorm"));
        assert_eq!(overrides.slug_for("other"), None);
        assert!(Overrides::parse("names = 3").is_err());
        let dir = tempfile::tempdir().unwrap();
        assert!(Overrides::load(&dir.path().join("missing.toml")).slug_for("x").is_none());
        std::fs::write(dir.path().join("bad.toml"), "not = [toml").unwrap();
        assert!(Overrides::load(&dir.path().join("bad.toml")).slug_for("x").is_none(), "invalid files are ignored");
    }

    #[test]
    fn classify_finds_the_platinum_side_and_extras() {
        let purchase = RawTrade {
            player_name: "P".into(),
            ee_timestamp: "1".into(),
            offered: vec![raw("Platinum", 43, None), raw("Mortus Lungfish (L)", 1, None)],
            received: vec![raw("Adaptation", 1, Some(10))],
        };
        let c = classify(&purchase).unwrap();
        assert_eq!((c.direction, c.platinum), (Direction::Purchase, 43));
        assert_eq!(c.goods, vec![raw("Adaptation", 1, Some(10))]);
        assert_eq!(c.extras, vec![raw("Mortus Lungfish (L)", 1, None)]);

        let sale = RawTrade { offered: purchase.received.clone(), received: purchase.offered.clone(), ..purchase.clone() };
        assert_eq!(classify(&sale).unwrap().direction, Direction::Sale);

        let swap = RawTrade { offered: vec![raw("Adaptation", 1, Some(10))], received: vec![raw("Primed Firestorm", 1, Some(10))], ..purchase.clone() };
        assert_eq!(classify(&swap).unwrap_err(), "no_platinum_side");
        let both = RawTrade { offered: vec![raw("Platinum", 5, None)], received: vec![raw("Platinum", 9, None)], ..purchase.clone() };
        assert_eq!(classify(&both).unwrap_err(), "no_platinum_side");
    }

    #[test]
    fn items_resolve_by_name_then_override_with_capped_ranks() {
        let index = index();
        let overrides = Overrides::parse("[names]\n\"Primed Fir\" = \"primed_firestorm\"\n").unwrap();
        let arcane = resolve_item(&raw("Arcane Nullifier \u{e0b9}\u{e0b9}\u{e0b9}\u{e0b9}\u{e0b9}\u{e0b9}\u{e0b9}", 1, Some(7)), &index, &overrides).unwrap();
        assert_eq!((arcane.slug.as_str(), arcane.matched_by.as_str()), ("arcane_nullifier", "name"));
        assert_eq!(arcane.sub_type, Some(SubType::rank(5)), "capped at max_rank");
        let part = resolve_item(&raw("wolf sledge HANDLE", 2, None), &index, &overrides).unwrap();
        assert_eq!((part.slug.as_str(), part.quantity, part.sub_type.is_none()), ("wolf_sledge_handle", 2, true));
        let overridden = resolve_item(&raw("Primed Fir", 1, Some(10)), &index, &overrides).unwrap();
        assert_eq!((overridden.slug.as_str(), overridden.matched_by.as_str()), ("primed_firestorm", "override"));
        assert!(resolve_item(&raw("Mortus Lungfish (L)", 1, None), &index, &overrides).is_none());
    }

    #[test]
    fn resolve_trade_keeps_the_unresolved_names() {
        let index = index();
        let trade = RawTrade {
            player_name: "P".into(),
            ee_timestamp: "1".into(),
            offered: vec![raw("Platinum", 30, None)],
            received: vec![raw("Wolf Sledge Handle", 1, None), raw("Mystery Thing", 1, None)],
        };
        let resolved = resolve_trade(&trade, &index, &Overrides::default()).unwrap();
        assert_eq!(resolved.unresolved, vec!["Mystery Thing".to_string()]);
        assert_eq!(resolved.resolution.direction, Some(Direction::Purchase));
        assert_eq!(resolved.resolution.platinum, 30);
        assert_eq!(resolved.resolution.items.len(), 1);
    }
}
