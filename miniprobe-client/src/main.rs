#![forbid(unsafe_code)]

use std::time::Duration;

use argh::FromArgs;
use simple_logger::SimpleLogger;

mod auth;
mod http_util;
mod query;

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

    // let mut querent = query::StatusQuerent::try_new(None)?;
    // let mut reconnect_timer = ReconnectTimer::new(
    //     Duration::from_secs(cfg.retry_minimum_interval),
    //     Duration::from_secs(cfg.retry_maximum_interval),
    // );

    let auth_resp = auth::auth(&cfg.token, &cfg.server_addr, cfg.tls).await?;

    println!("{:?}", auth_resp);

    Ok(())
}

// fn connect_to_server(server_addr: &str) -> anyhow::Result<WebSocket<MaybeTlsStream<TcpStream>>> {
//     let request = Request::builder().uri(server_addr).body(())?;
//     let (socket, response) = connect(request)?;

//     log::info!("Connection to {} established.", server_addr);

//     log::debug!("With response headers:");
//     for (header, _value) in response.headers() {
//         log::debug!("* {header}");
//     }

//     Ok(socket)
// }

// fn report_status(
//     querent: &mut StatusQuerent,
//     socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
//     scrape_interval: Duration,
// ) -> anyhow::Result<()> {
//     // Initial static report
//     socket.send(Message::binary(to_allocvec(&msg::Message::ReportStatic(
//         StatusQuerent::query_static(),
//     ))?))?;
//     log::debug!("Sent static status to server.");

//     loop {
//         // Interval dynamic report
//         socket.send(Message::binary(to_allocvec(&msg::Message::ReportDynamic(
//             querent.query_dynamic(),
//         ))?))?;
//         log::debug!("Sent dynamic status to server.");

//         thread::sleep(scrape_interval);
//     }
// }

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

    fn wait(&mut self) {
        std::thread::sleep(self.curr_interval);
        self.curr_interval = (self.curr_interval * 2).min(self.maximal_interval);
    }

    fn reset(&mut self) {
        self.curr_interval = self.minimal_interval;
    }

    fn interval(&self) -> Duration {
        self.curr_interval
    }
}
