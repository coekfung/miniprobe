use axum::extract::ws::{CloseFrame, Message, WebSocket, close_code};
use futures_util::SinkExt;
use miniprobe_proto::DynamicMetrics;
use sqlx::SqlitePool;
use tokio_util::sync::CancellationToken;
use tracing::{debug, trace};

use crate::{AppState, route::sessions::SessionLock};
pub async fn handle_socket<'a>(
    mut socket: WebSocket,
    state: AppState,
    SessionLock(session): SessionLock,
) {
    let _tracker_token = state.ws_graceful_shutdown.tracker.token();
    let cancellation_token = state.ws_graceful_shutdown.token.child_token().child_token();

    let session = session.try_own();

    match session {
        Some(session) => {
            let session_id = session.read().await.id;
            debug!("websocket connected");
            let mut controller = IngressController {
                db: state.pool.clone(),
                ws: socket,
                cancellation_token,
                session_id,
            };

            while controller.next().await {}
            controller.ws.close().await.ok();
            debug!("websocket disconnected");
        }
        None => {
            debug!("conflict websocket connection for session");
            socket
                .send(Message::Close(
                    IngressWsError::SessionMutexPoisoned.into_close_frame(),
                ))
                .await
                .ok();
            socket.close().await.ok();
        }
    }
}

struct IngressController {
    db: SqlitePool,
    ws: WebSocket,
    cancellation_token: CancellationToken,
    session_id: i64,
}

impl IngressController {
    async fn close<T: IntoCloseFrame>(&mut self, msg: T) -> anyhow::Result<()> {
        let msg = msg.into_close_frame();
        match msg {
            Some(CloseFrame { code, ref reason }) if code != close_code::NORMAL => {
                debug!(
                    code,
                    %reason,
                    "closing websocket with error"
                );
            }
            _ => {}
        }
        self.ws.send(Message::Close(msg)).await?;
        Ok(())
    }

    async fn next(&mut self) -> bool {
        tokio::select! {
            msg = self.ws.recv() => {
                let msg = match msg {
                    Some(Ok(m)) => m,
                    Some(Err(e)) => {
                        let reason = IngressWsError::Internal(e.to_string());
                        self.close(reason).await.ok();
                        return false;
                    }
                    None => {
                        return false; // connection closed
                    }
                };

                if let Err(e) = self.process_msg(msg).await {
                    self.close(e).await.ok();
                    return false;
                }
                return true;
            }
            _ = self.cancellation_token.cancelled() => {
                self.close(IngressWsError::Shutdown).await.ok();
                return false;
            }
        }
    }

    async fn process_msg(&mut self, msg: Message) -> Result<(), IngressWsError> {
        match msg {
            Message::Close(Some(CloseFrame { code, reason })) => {
                trace!(
                    code,
                    %reason,
                    "websocket closed with frame"
                );
            }
            Message::Binary(bytes) => {
                trace!("received binary: {:?}", String::from_utf8_lossy(&bytes));

                let metrics: DynamicMetrics = postcard::from_bytes(&bytes)
                    .map_err(|e| IngressWsError::Internal(e.to_string()))?;

                trace!("decoded into metrics: {:?}", metrics);

                self.write_metrics_to_db(metrics)
                    .await
                    .map_err(|e| IngressWsError::Internal(e.to_string()))?;
            }
            Message::Text(_) => {
                return Err(IngressWsError::UnexpectedMessage);
            }
            _ => {} // ignore other messages
        }
        Ok(())
    }

    async fn write_metrics_to_db(&mut self, metrics: DynamicMetrics) -> anyhow::Result<()> {
        let mut tx = self.db.begin().await?;
        let sample_time = metrics.sample_time as i64; // will overflow in 2038, but who cares

        let session_data_id = sqlx::query!(
            r#"
            INSERT INTO session_data (session_id, sample_time)
            VALUES (?, ?)
            RETURNING id
            "#,
            self.session_id,
            sample_time,
        )
        .fetch_one(&mut *tx)
        .await?
        .id;

        // cpu metrics
        for (i, cpu_metric) in metrics.cpu.into_iter().enumerate() {
            let i = i as i64;
            sqlx::query!(
                r#"
                INSERT INTO session_data_cpu (session_data_id, cpu_id, cpu_usage)
                VALUES (?, ?, ?)
                "#,
                session_data_id,
                i,
                cpu_metric.usage,
            )
            .execute(&mut *tx)
            .await?;
        }

        // memory metrics
        {
            // will someone use that much memory? I doubt it.
            let (total, used) = (metrics.memory.total as i64, metrics.memory.used as i64);
            let (swap_total, swap_used) = (
                metrics.memory.swap_total as i64,
                metrics.memory.swap_used as i64,
            );
            sqlx::query!(
                r#"
                INSERT INTO session_data_memory (session_data_id, total, used, swap_total, swap_used)
                VALUES (?, ?, ?, ?, ?)
                "#,
                session_data_id,
                total,
                used,
                swap_total,
                swap_used,
            )
            .execute(&mut *tx)
            .await?;
        }

        // network metrics
        {
            let (rx_bytes, tx_bytes) = (
                metrics.network.rx_bytes.map(|i| i as i64),
                metrics.network.tx_bytes.map(|i| i as i64),
            );

            sqlx::query!(
                r#"
                INSERT INTO session_data_network (session_data_id, ifname, rx_bytes, tx_bytes)
                VALUES (?, ?, ?, ?)
                "#,
                session_data_id,
                metrics.network.ifname,
                rx_bytes,
                tx_bytes,
            )
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }
}

trait IntoCloseFrame {
    fn into_close_frame(self) -> Option<CloseFrame>;
}

impl IntoCloseFrame for Option<CloseFrame> {
    fn into_close_frame(self) -> Option<CloseFrame> {
        self
    }
}

#[derive(Debug, thiserror::Error)]
enum IngressWsError {
    #[error("session mutex poisoned")]
    SessionMutexPoisoned,
    #[error("server is shutting down")]
    Shutdown,
    #[error("unexpected message from client")]
    UnexpectedMessage,
    #[error("internal error: {0}")]
    Internal(String),
}

impl IntoCloseFrame for IngressWsError {
    fn into_close_frame(self) -> Option<CloseFrame> {
        Some(match self {
            IngressWsError::SessionMutexPoisoned => CloseFrame {
                code: close_code::ERROR,
                reason: "session mutex poisoned".into(),
            },
            IngressWsError::Shutdown => CloseFrame {
                code: close_code::AWAY,
                reason: "server shutting down".into(),
            },
            IngressWsError::UnexpectedMessage => CloseFrame {
                code: close_code::UNSUPPORTED,
                reason: "unexpected message from client".into(),
            },
            IngressWsError::Internal(reason) => CloseFrame {
                code: close_code::ERROR,
                reason: format!("internal error: {}", reason).into(),
            },
        })
    }
}
