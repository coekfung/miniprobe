use serde::{Deserialize, Serialize};

use crate::{DynamicStatus, StaticStatus};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    ReportStatic(StaticStatus),
    ReportDynamic(DynamicStatus),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthReqMessage {
    pub token: String,
    pub system_info: StaticStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthRespMessage {
    pub session_token: SessionToken,
    pub scrape_interval: u64,
}

#[derive(PartialEq, Eq, Hash, Clone, Serialize, Deserialize)]
pub struct SessionToken([u8; 32]);

impl std::fmt::Debug for SessionToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SessionToken({:?})", String::from_utf8_lossy(&self.0))
    }
}

impl std::fmt::Display for SessionToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", String::from_utf8_lossy(&self.0))
    }
}

#[cfg(feature = "rand")]
impl SessionToken {
    pub fn random() -> Self {
        use rand::{Rng, distr::Alphanumeric};

        let mut token_bytes = [0; 32];

        rand::rng()
            .sample_iter(&Alphanumeric)
            .take(32)
            .enumerate()
            .for_each(|(i, c)| {
                token_bytes[i] = c;
            });

        SessionToken(token_bytes)
    }
}
