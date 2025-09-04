mod metrics;
mod sessions;

use axum::Json;
use serde_json::{Value, json};

pub use metrics::metric_ingress_ws;
pub use sessions::SessionManager;
pub use sessions::create_session;

pub async fn health() -> Json<Value> {
    Json(json!({"status": "ok"}))
}
