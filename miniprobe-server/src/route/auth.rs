use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use miniprobe_proto::msg::{AuthReqMessage, AuthRespMessage};

use crate::{
    AppState, CLINET_TOKEN_LENGTH, index_client_token, postcard::Postcard, session::Session,
};

pub async fn auth(
    State(state): State<AppState>,
    Postcard(AuthReqMessage { token, system_info }): Postcard<AuthReqMessage>,
) -> Result<Postcard<AuthRespMessage>, AuthError> {
    let system_status = system_info.system;
    let mut tx = state.pool.begin().await?;

    if token.len() != CLINET_TOKEN_LENGTH {
        return Err(AuthError::InvalidToken(token));
    }

    let token_idx = index_client_token(&token);

    // check if token exists in the database
    let record = sqlx::query!(
        "SELECT id, token_hash FROM clients WHERE token_idx = $1",
        token_idx
    )
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .find(|r| password_auth::verify_password(&token, &r.token_hash).is_ok());

    let client_id = if let Some(record) = record {
        record.id
    } else {
        return Err(AuthError::InvalidToken(token));
    };

    // create a new session
    let session = sqlx::query_as!(
        Session,
        "INSERT INTO sessions \
            (client_id, system_name, kernel_version, os_version, host_name, cpu_arch) \
            VALUES ($1, $2, $3, $4, $5, $6) \
            RETURNING id",
        client_id,
        system_status.system_name,
        system_status.kernel_version,
        system_status.os_version,
        system_status.host_name,
        system_status.cpu_arch
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AuthError::CreateSessionError)?;

    let token = state.session_mgr.write().await.add_session(session);

    tx.commit().await?;

    Ok(Postcard(AuthRespMessage {
        session_token: token,
        scrape_interval: 5,
    }))
}

#[derive(thiserror::Error, Debug)]
pub enum AuthError {
    #[error("Invalid token: {0}")]
    InvalidToken(String),
    #[error("Database error: {0}")]
    DatabaseError(#[from] sqlx::Error),
    #[error("Failed to create session")]
    CreateSessionError,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        match self {
            AuthError::InvalidToken(_) => {
                (StatusCode::UNAUTHORIZED, self.to_string()).into_response()
            }
            _ => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()).into_response(),
        }
    }
}
