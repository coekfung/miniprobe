#![forbid(unsafe_code)]

use std::time::Duration;

use argh::FromArgs;
use miniprobe_proto::msg::CreateSessionResp;
use simple_logger::SimpleLogger;
use tokio::time::sleep;

mod egress;
mod http_util;
mod query;
mod session;

#[derive(FromArgs, Debug)]
#[argh(description = "A lightweight system status probe client.")]
struct ClientConfig {
    #[argh(positional, description = "authentication token")]
    pub token: String,
    #[argh(
        option,
        short = 'a',
        default = "\"127.0.0.1:8000\".to_string()",
        description = "server address to connect to"
    )]
    pub server_addr: String,
    #[argh(
        switch,
        short = 't',
        description = "use TLS to connect to server (https/wss instead of http/ws)"
    )]
    pub tls: bool,
    #[argh(
        switch,
        short = '6',
        description = "prefer IPv6 when resolving server address"
    )]
    pub prefer_ipv6: bool,
    #[argh(
        option,
        default = "1",
        description = "minimum interval between two connection retries in seconds"
    )]
    pub retry_minimum_interval: u64, // in seconds
    #[argh(
        option,
        default = "300",
        description = "maximum interval between two connection retries in seconds"
    )]
    pub retry_maximum_interval: u64, // in seconds
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    SimpleLogger::new().env().init()?;

    let cfg: ClientConfig = argh::from_env();
    log::debug!("Client config: {cfg:#?}");

    let mut querent = query::MetricsQuerent::try_new(None)?;
    let mut reconnect_timer = ReconnectTimer::new(
        Duration::from_secs(cfg.retry_minimum_interval),
        Duration::from_secs(cfg.retry_maximum_interval),
    );

    loop {
        let res: anyhow::Result<()> = async {
            let CreateSessionResp {
                session_token,
                scrape_interval,
            } = session::create_session(&cfg.token, &cfg.server_addr, cfg.tls, cfg.prefer_ipv6)
                .await?;
            reconnect_timer.reset();

            egress::metrics_egress(
                &mut querent,
                Duration::from_secs(scrape_interval),
                &session_token,
                &cfg.server_addr,
                cfg.tls,
                cfg.prefer_ipv6,
            )
            .await?;
            Ok(())
        }
        .await;

        if let Err(e) = res {
            log::warn!("Error occurred: {e}");
            log::info!(
                "Reconnecting in {} seconds...",
                reconnect_timer.interval().as_secs()
            );
            reconnect_timer.wait().await;
        } else {
            return Ok(()); // means graceful shutdown
        }
    }
}

struct ReconnectTimer {
    minimal_interval: Duration,
    maximal_interval: Duration,
    curr_interval: Duration,
}

impl ReconnectTimer {
    fn new(minimal_interval: Duration, maximal_interval: Duration) -> Self {
        debug_assert!(minimal_interval <= maximal_interval);

        Self {
            minimal_interval,
            maximal_interval,
            curr_interval: minimal_interval,
        }
    }

    async fn wait(&mut self) {
        sleep(self.curr_interval).await;
        self.curr_interval = (self.curr_interval * 2).min(self.maximal_interval);
    }

    fn reset(&mut self) {
        self.curr_interval = self.minimal_interval;
    }

    fn interval(&self) -> Duration {
        self.curr_interval
    }
}
