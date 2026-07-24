use anyhow::Result;
use clap::{Parser, Subcommand};
use helper_api::{ApiState, serve as serve_api};
use helper_core::Config;
use helper_modules::EntitlementClient;
use helper_store::Store;
use std::time::Instant;
use tokio::{select, signal};

#[derive(Debug, Parser)]
#[command(name = "vozen-helper", about = "Vozen Helper Rust runtime")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Serve,
    Migrate,
    Doctor,
    Benchmark {
        #[arg(long, default_value_t = 10_000)]
        iterations: u32,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let log_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(log_filter)
        .json()
        .init();
    let cli = Cli::parse();
    let config = Config::from_env()?;
    config.validate()?;
    let store = Store::open(&config.database_url)?;
    match cli.command {
        Command::Migrate => {
            store.migrate()?;
            println!("migrations applied");
        }
        Command::Doctor => {
            println!("helper rust doctor: ok");
        }
        Command::Benchmark { iterations } => {
            let started = Instant::now();
            for index in 0..iterations {
                let _ = store.consume_quota(
                    "benchmark-guild",
                    &format!("user-{index}"),
                    "workflow_runs",
                    u64::MAX,
                    chrono::Utc::now(),
                )?;
            }
            let elapsed = started.elapsed();
            let rate = f64::from(iterations) / elapsed.as_secs_f64().max(0.000_001);
            println!(
                "sqlite quota benchmark: {iterations} ops in {:?} ({rate:.0} ops/s)",
                elapsed
            );
        }
        Command::Serve => {
            let api = serve_api(
                &config.bind_addr,
                ApiState {
                    store: store.clone(),
                    session_secret: config.session_secret.clone(),
                    oauth_client_id: config.oauth_client_id.clone(),
                    oauth_client_secret: config.oauth_client_secret.clone(),
                    oauth_redirect_uri: config.oauth_redirect_uri.clone(),
                    allow_legacy_session: config.allow_legacy_session,
                    allowed_origin: std::env::var("HELPER_ALLOWED_ORIGIN").ok(),
                    entitlements: EntitlementClient::new(
                        config.entitlement_url.clone(),
                        config.entitlement_secret.clone(),
                    ),
                },
            );
            let discord = async {
                if config.api_only {
                    signal::ctrl_c().await.map_err(anyhow::Error::from)
                } else {
                    helper_discord::run(&config).await
                }
            };
            select! { result = api => result?, result = discord => result?, _ = signal::ctrl_c() => {} }
        }
    }
    Ok(())
}
