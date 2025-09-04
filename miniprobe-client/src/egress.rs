use std::time::Duration;

use bytes::BytesMut;
use futures_util::{SinkExt, StreamExt};
use http::{HeaderValue, header};
use log::{debug, warn};
use miniprobe_proto::msg::SessionToken;
use tokio::time::{Instant, sleep_until};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest, protocol::CloseFrame};
use tokio_util::sync::CancellationToken;

use crate::{http_util::connect_tls, query::MetricsQuerent};

pub async fn metrics_egress(
    querent: &mut MetricsQuerent,
    scrape_interval: Duration,
    session_token: &SessionToken,
    server_addr: &str,
    tls: bool,
    prefer_ipv6: bool,
) -> anyhow::Result<()> {
    let mut req = format!(
        "{}://{server_addr}/ws/v1/metrics/ingress",
        if tls { "wss" } else { "ws" }
    )
    .into_client_request()?;
    req.headers_mut().insert(
        header::AUTHORIZATION,
        HeaderValue::from_str(format!("Bearer {session_token}").as_str())?,
    );

    let stream = connect_tls(&req, tls, prefer_ipv6).await?;

    let (socket, _) = tokio_tungstenite::client_async(req, stream).await?;

    let (mut write, mut read) = socket.split();

    let read_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = read.next().await {
            match msg {
                Message::Close(Some(CloseFrame { code, reason })) => {
                    warn!("WebSocket closed by server: code={code:?}, reason={reason}");
                }
                _ => {} // we dont care
            }
        }
    });

    let shutdown_token = CancellationToken::new();
    tokio::spawn({
        let shutdown_token = shutdown_token.clone();
        async move {
            tokio::signal::ctrl_c()
                .await
                .expect("failed to listen for ctrl-c");
            shutdown_token.cancel();
        }
    });

    loop {
        let current_time = Instant::now();
        let metrics = querent.query_dynamic();
        let res: anyhow::Result<()> = async {
            write
                .send(Message::Binary(
                    postcard::to_extend(&metrics, BytesMut::new())?.freeze(),
                ))
                .await?;
            Ok(())
        }
        .await;

        // delay error propagation
        if let Err(e) = res {
            let _ = tokio::join!(write.close(), read_task);
            return Err(e);
        }

        debug!("metrics egress sucessfully");

        // wait scrape interval or ctrl-c
        tokio::select! {
           _ = shutdown_token.cancelled() => {
               let _ = tokio::join!(write.close(), read_task);
               return Ok(());
           }
           _ = sleep_until(current_time + scrape_interval) => { /* continue */ }
        }
    }
}
