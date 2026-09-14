use crate::app::{Settings, User};
use wf_market::client::Authenticated as WFAuthenticated;
use wf_market::types::websocket::WsClient;
use wf_market::Client as WFClient;

#[derive(Clone)]
pub struct AppState {
    pub user: User,
    pub settings: Settings,
    pub wfm_client: WFClient<WFAuthenticated>,
    pub is_development: bool,
    pub is_pre_release: bool,
    pub use_temp_db: bool,
    pub wfm_socket: Option<WsClient>,
}
