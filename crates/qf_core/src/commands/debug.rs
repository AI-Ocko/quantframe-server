use entity::{
    dto::{PaginatedResult, PaginationQueryDto},
    enums::FieldChange,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use utils::*;
use wf_market::enums::OrderType;


pub async fn debug_get_wfm_state() -> Result<Value, Error> {
    let app = crate::utils::modules::states::app_mutex();
    let app = app.lock()?.clone();
    let orders = app.wfm_client.order().cache_orders();
    let tracking = app.wfm_client.get_tracking().clone();
    let mut payload = json!({
      "user_orders": json!(orders),
      "order_limit": app.wfm_client.order().get_order_limit(),
      "tracking": json!(tracking),
      "limiters": {}
    });
    let per_rate_limit = app.wfm_client.get_per_route_limiter().clone();
    for (key, route) in per_rate_limit.lock()?.iter() {
        payload["limiters"][key] = json!({"limit": route.quota_type.current_limit(), "wait_time_sec": route.wait_time_sec, "quota_type": route.quota_type.quota_type()});
    }
    Ok(payload)
}

