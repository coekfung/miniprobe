use std::{
    net::{IpAddr, SocketAddr},
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use anyhow::anyhow;
use axum::{
    Router,
    routing::{get, post},
};
use clap::{Parser, Subcommand};
use confique::Config;
use sha2::{Digest, Sha256};
use sqlx::{SqlitePool, sqlite::SqliteConnectOptions};
use tokio::{net::TcpListener, signal, sync::RwLock};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tower_http::{timeout::TimeoutLayer, trace::TraceLayer};
use tracing::{info, trace};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::route::SessionManager;

mod admin;
mod lock;
mod postcard;
mod route;

const CLINET_TOKEN_LENGTH: usize = 16;

#[derive(Debug, Parser)]
#[command(name = "miniprobe-server")]
struct Cli {
    #[arg(short, long, value_name = "FILE", help = "Path to config file")]
    config_path: Option<String>,
    #[command(subcommand)]
    commands: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Run the server
    Serve,

    /// Administrative commands
    #[command(subcommand)]
    Admin(admin::AdminCommands),
}

#[derive(Config, Debug)]
struct Conf {
    /// Port to listen on
    #[config(default = 8000)]
    port: u16,

    /// Bind address
    #[config(default = "127.0.0.1")]
    address: IpAddr,

    /// Database URL
    #[config(default = "sqlite://db.sqlite")]
    database_url: String,
}

fn config(path: &str) -> anyhow::Result<Conf> {
    Conf::builder()
        .env()
        .file(path)
        .load()
        .map_err(|e| e.into())
}

#[derive(Clone, Debug)]
pub(crate) struct AppState {
    pub session_mgr: Arc<RwLock<SessionManager>>,
    pub pool: SqlitePool,
    pub ws_graceful_shutdown: WebsocketGracefule,
}

#[derive(Clone, Debug)]
struct WebsocketGracefule {
    pub token: CancellationToken,
    pub tracker: TaskTracker,
}

fn app(state: AppState) -> Router {
    Router::new()
        .route("/health", get(route::health))
        // .route("/auth", post(route::auth))
        .nest(
            "/api/v1",
            Router::new().route("/sessions", post(route::create_session)),
        )
        .nest(
            "/ws/v1",
            Router::new().route("/metrics/ingress", get(route::metric_ingress_ws)),
        )
        .layer((
            TraceLayer::new_for_http(),
            // Prevent requests to hang forever
            TimeoutLayer::new(Duration::from_secs(60)),
        ))
        .with_state(state)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let cli = Cli::parse();
    trace!("using command line arguments {:?}", cli);

    let config = config(&cli.config_path.unwrap_or("config.toml".to_owned()))?;
    trace!("using config {:?}", config);

    let db_opts = SqliteConnectOptions::from_str(&config.database_url)?.create_if_missing(true);
    let pool = SqlitePool::connect_with(db_opts).await?;
    sqlx::migrate!()
        .run(&pool)
        .await
        .map_err(|e| anyhow!("failed to initialize SQLx database: {e}"))?;

    match cli.commands {
        Commands::Serve => {
            let addr = SocketAddr::from((config.address, config.port));
            info!("listening on {addr}");
            let listener = TcpListener::bind(addr).await?;

            let state = AppState {
                session_mgr: Arc::new(RwLock::new(SessionManager::new())),
                pool: pool.clone(),
                ws_graceful_shutdown: WebsocketGracefule {
                    token: CancellationToken::new(),
                    tracker: TaskTracker::new(),
                },
            };

            axum::serve(listener, app(state.clone()))
                .with_graceful_shutdown(shutdown_signal(state.ws_graceful_shutdown.token.clone()))
                .await?;

            let ws_tracker = state.ws_graceful_shutdown.tracker.clone();
            ws_tracker.close();

            trace!("waiting {} websocket connection shutdown", ws_tracker.len());
            ws_tracker.wait().await;
        }
        Commands::Admin(command) => admin::admin(command, pool.clone()).await?,
    }

    trace!("closing database connection");
    pool.close().await;

    Ok(())
}

fn init_tracing() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                #[cfg(debug_assertions)]
                let default_log_level = format!(
                    "{}=debug,tower_http=debug,axum=trace",
                    env!("CARGO_CRATE_NAME")
                )
                .into();

                #[cfg(not(debug_assertions))]
                let default_log_level = format!(
                    "{}=info,tower_http=info,axum=info",
                    env!("CARGO_CRATE_NAME")
                )
                .into();

                default_log_level
            }),
        )
        .with(tracing_subscriber::fmt::layer().without_time())
        .init();
}

async fn shutdown_signal(ws_token: CancellationToken) {
    let _ws_shutdown_guard = ws_token.drop_guard();

    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl-C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}

#[inline]
fn index_client_token(token: &str) -> u32 {
    let token_idx = Sha256::digest(token[..4].as_bytes())
        .into_iter()
        .take(4)
        .fold(0, |acc, b| (acc << 8) | b as u32);

    token_idx
}
