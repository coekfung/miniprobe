mod auth;

use axum::Json;
use serde_json::{Value, json};

pub use auth::auth;

pub async fn health() -> Json<Value> {
    Json(json!({"status": "ok"}))
}
