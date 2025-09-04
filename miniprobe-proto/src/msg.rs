use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::StaticMetrics;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSessionReq {
    pub token: String,
    pub system_info: StaticMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSessionResp {
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

impl FromStr for SessionToken {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = s.as_bytes();
        if bytes.len() != 32 {
            return Err("SessionToken must be 32 bytes long");
        }

        let mut token_bytes = [0; 32];
        token_bytes.copy_from_slice(&bytes[0..32]);

        Ok(SessionToken(token_bytes))
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
