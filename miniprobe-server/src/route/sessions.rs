use axum::{
    extract::{FromRequestParts, State},
    http::{StatusCode, request::Parts},
    response::{IntoResponse, Response},
};
use axum_auth::AuthBearer;
use miniprobe_proto::msg::{CreateSessionReq, CreateSessionResp, SessionToken};
use std::{collections::HashMap, sync::Arc};
use tracing::debug;

use crate::{
    AppState, CLINET_TOKEN_LENGTH, index_client_token, lock::SharedOwnable, postcard::Postcard,
};

pub async fn create_session(
    State(state): State<AppState>,
    Postcard(CreateSessionReq { token, system_info }): Postcard<CreateSessionReq>,
) -> Result<Postcard<CreateSessionResp>, CreateSessionError> {
    let system_status = system_info.system;
    let mut tx = state.pool.begin().await?;

    if token.len() != CLINET_TOKEN_LENGTH {
        return Err(CreateSessionError::InvalidToken(token));
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
        return Err(CreateSessionError::InvalidToken(token));
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
    .fetch_one(&mut *tx)
    .await?;

    let token = state.session_mgr.write().await.add_session(session);

    tx.commit().await?;

    debug!(client_id, ?token, "session created");

    Ok(Postcard(CreateSessionResp {
        session_token: token,
        scrape_interval: 5,
    }))
}

#[derive(thiserror::Error, Debug)]
pub enum CreateSessionError {
    #[error("Invalid token: {0}")]
    InvalidToken(String),
    #[error("Database error: {0}")]
    DatabaseError(#[from] sqlx::Error),
}

impl IntoResponse for CreateSessionError {
    fn into_response(self) -> Response {
        match self {
            CreateSessionError::InvalidToken(_) => {
                (StatusCode::UNAUTHORIZED, self.to_string()).into_response()
            }
            _ => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()).into_response(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct SessionManager {
    authed_sessions: HashMap<SessionToken, Arc<SharedOwnable<Session>>>,
}

impl SessionManager {
    pub fn new() -> Self {
        SessionManager {
            authed_sessions: HashMap::new(),
        }
    }

    pub fn add_session(&mut self, session: Session) -> SessionToken {
        // ensure the token is unique
        let token = loop {
            let token = SessionToken::random();
            if !self.authed_sessions.contains_key(&token) {
                break token;
            }
        };

        self.authed_sessions
            .insert(token.clone(), SharedOwnable::new(session));

        token
    }

    pub fn get_session(&self, token: &SessionToken) -> Option<Arc<SharedOwnable<Session>>> {
        self.authed_sessions.get(token).cloned()
    }
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct Session {
    pub id: i64,
}

#[derive(Clone, Debug)]
pub struct SessionLock(pub Arc<SharedOwnable<Session>>);

#[derive(Debug, thiserror::Error)]
pub enum SessionMutexRejection {
    #[error("Invalid session token")]
    InvalidToken,
    #[error("Auth error: {}", 0.1)]
    BearerRejection(axum_auth::Rejection),
}

impl IntoResponse for SessionMutexRejection {
    fn into_response(self) -> Response {
        match self {
            SessionMutexRejection::InvalidToken => {
                (StatusCode::UNAUTHORIZED, self.to_string()).into_response()
            }
            Self::BearerRejection(inner) => inner.into_response(),
        }
    }
}

impl FromRequestParts<AppState> for SessionLock {
    type Rejection = SessionMutexRejection;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let AuthBearer(token) = AuthBearer::from_request_parts(parts, state)
            .await
            .map_err(|e| SessionMutexRejection::BearerRejection(e))?;

        let session = state
            .session_mgr
            .read()
            .await
            .get_session(
                &token
                    .parse()
                    .map_err(|_| SessionMutexRejection::InvalidToken)?,
            )
            .ok_or(SessionMutexRejection::InvalidToken)?;

        Ok(SessionLock(session))
    }
}
