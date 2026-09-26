use serde::Deserialize;
use utils::{get_location, Error};
use wf_market::{enums::StatusType, types::OrderWithUser};

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct V2OrderUser {
    pub id: String,
    #[serde(default)]
    pub ingame_name: String,
    /// `offline`, `online` or `ingame`.
    #[serde(default)]
    pub status: String,
}

/// One order from `GET /v2/orders/item/{slug}` (spec §5.4, verified 2026-09-14).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct V2Order {
    pub id: String,
    /// `sell` or `buy`.
    #[serde(rename = "type")]
    pub side: String,
    pub platinum: i64,
    pub quantity: i64,
    pub rank: Option<i64>,
    pub charges: Option<i64>,
    pub subtype: Option<String>,
    pub amber_stars: Option<i64>,
    pub cyan_stars: Option<i64>,
    #[serde(default = "visible_by_default")]
    pub visible: bool,
    pub item_id: String,
    pub user: V2OrderUser,
}

fn visible_by_default() -> bool {
    true
}

impl V2Order {
    pub fn sub_type_key(&self) -> String {
        sub_type_key(
            self.rank,
            self.charges,
            self.subtype.as_deref(),
            self.amber_stars,
            self.cyan_stars,
        )
    }

    pub fn is_ingame(&self) -> bool {
        self.user.status == "ingame"
    }
}

/// The `sub_type` column value (amendment B4): `""` without rank or variant fields,
/// otherwise `key=value` parts in the order rank, charges, subtype, amber, cyan, joined by `;`.
pub fn sub_type_key(
    rank: Option<i64>,
    charges: Option<i64>,
    variant: Option<&str>,
    amber_stars: Option<i64>,
    cyan_stars: Option<i64>,
) -> String {
    let mut parts = Vec::new();
    if let Some(rank) = rank {
        parts.push(format!("rank={rank}"));
    }
    if let Some(charges) = charges {
        parts.push(format!("charges={charges}"));
    }
    if let Some(variant) = variant {
        parts.push(format!("subtype={variant}"));
    }
    if let Some(amber) = amber_stars {
        parts.push(format!("amber={amber}"));
    }
    if let Some(cyan) = cyan_stars {
        parts.push(format!("cyan={cyan}"));
    }
    parts.join(";")
}

/// The same order as the trader's wf-market client parsed it (spec P23).
pub fn from_wfm(o: &OrderWithUser) -> V2Order {
    let order = &o.order;
    V2Order {
        id: order.id.clone(),
        side: order.order_type.to_string(),
        platinum: order.platinum.into(),
        quantity: order.quantity.into(),
        rank: order.subtype.rank,
        charges: order.subtype.charges,
        subtype: order.subtype.subtype.clone(),
        amber_stars: order.subtype.amber_stars,
        cyan_stars: order.subtype.cyan_stars,
        visible: order.visible,
        item_id: order.item_id.clone(),
        user: V2OrderUser {
            id: o.user.id.clone(),
            ingame_name: o.user.name.clone(),
            status: match o.user.status {
                StatusType::InGame => "ingame",
                StatusType::Online => "online",
                StatusType::Offline => "offline",
            }
            .to_string(),
        },
    }
}

#[derive(Deserialize)]
struct OrdersResponse {
    data: Vec<V2Order>,
}

pub fn parse_orders_response(json: &str) -> Result<Vec<V2Order>, Error> {
    serde_json::from_str::<OrdersResponse>(json)
        .map(|r| r.data)
        .map_err(|e| {
            Error::new(
                "Collector:Parse",
                format!("Invalid /v2/orders/item response: {}", e),
                get_location!(),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMALL: &str = include_str!("../../tests/fixtures/orders_small.json");
    const ARCANE: &str = include_str!("../../tests/fixtures/orders_arcane_energize.json");
    const RELIC: &str = include_str!("../../tests/fixtures/orders_axi_a1_relic.json");
    const AYATAN: &str = include_str!("../../tests/fixtures/orders_ayatan_anasa_sculpture.json");

    #[test]
    fn parses_the_small_fixture() {
        let orders = parse_orders_response(SMALL).unwrap();
        assert_eq!(orders.len(), 7);
        assert_eq!(orders[4].side, "buy");
        assert_eq!(orders[4].user.id, "u4");
        assert!(orders[0].is_ingame());
        assert!(!orders[1].is_ingame());
        assert!(!orders[6].visible);
        assert_eq!(orders[3].sub_type_key(), "rank=5");
    }

    #[test]
    fn sub_type_key_uses_a_fixed_order() {
        assert_eq!(sub_type_key(None, None, None, None, None), "");
        assert_eq!(sub_type_key(Some(5), None, None, None, None), "rank=5");
        assert_eq!(sub_type_key(None, None, Some("intact"), None, None), "subtype=intact");
        assert_eq!(sub_type_key(None, None, None, Some(0), Some(1)), "amber=0;cyan=1");
        assert_eq!(sub_type_key(Some(0), Some(3), None, None, None), "rank=0;charges=3");
    }

    #[test]
    fn recorded_responses_parse_with_the_expected_sub_types() {
        for (json, prefix) in [(ARCANE, "rank="), (RELIC, "subtype="), (AYATAN, "amber=")] {
            let orders = parse_orders_response(json).unwrap();
            assert!(!orders.is_empty());
            let item_id = &orders[0].item_id;
            for order in &orders {
                assert_eq!(&order.item_id, item_id);
                assert!(order.sub_type_key().starts_with(prefix), "{}", order.sub_type_key());
                assert!(order.side == "sell" || order.side == "buy");
            }
        }
    }

    #[test]
    fn from_wfm_matches_the_v2_parse_of_the_same_fixture() {
        let v2 = parse_orders_response(SMALL).unwrap();
        let mut json: serde_json::Value = serde_json::from_str(SMALL).unwrap();
        // wf-market's `UserShort` requires `reputation` (the live API sends it); the fixture omits it.
        for order in json["data"].as_array_mut().unwrap() {
            order["user"]["reputation"] = 0.into();
        }
        let wfm: Vec<OrderWithUser> = serde_json::from_value(json["data"].take()).unwrap();
        assert_eq!(wfm.iter().map(from_wfm).collect::<Vec<_>>(), v2);
    }

    #[test]
    fn invalid_json_is_an_error() {
        assert!(parse_orders_response("{\"data\": 1}").is_err());
    }
}
