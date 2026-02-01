// _Aud.io/offline-intelligence/crates/src/main.rs

use offline_intelligence::{config::Config, run_thread_server};
use dotenvy::dotenv;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    
    let cfg = Config::from_env()?;
    
    println!("Aud.io server starting (thread-based architecture)");
    run_thread_server(cfg).await
}