use axum::{
    extract::{State, WebSocketUpgrade},
    response::Response,
};
use tracing::{Instrument, debug_span};

use crate::{AppState, route::sessions::SessionLock};

mod ingress;

pub async fn metric_ingress_ws(
    session: SessionLock,
    State(state): State<AppState>,
    ws: WebSocketUpgrade,
) -> Response {
    let session_id = session.0.read().await.id;
    ws.on_upgrade(move |socket| {
        ingress::handle_socket(socket, state, session)
            .instrument(debug_span!("ingress_ws", session_id))
    })
}
