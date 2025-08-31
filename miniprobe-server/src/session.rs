use std::{collections::HashMap, sync::Arc};

use miniprobe_proto::msg::SessionToken;
use tokio::sync::Mutex;

#[derive(Clone, Debug)]
pub struct SessionManager {
    authed_sessions: HashMap<SessionToken, Arc<Mutex<Session>>>,
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
            .insert(token.clone(), Arc::new(Mutex::new(session)));

        token
    }
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct Session {
    pub id: i64,
}
