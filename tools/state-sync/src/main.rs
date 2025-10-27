
use clap::Parser;
use anyhow::Result;
use serde::{Serialize, Deserialize};
use exec::AccountState;

#[derive(Parser, Debug, Clone)]
struct Opts {
    #[arg(long)] src: String,
    #[arg(long)] dst: String,
    #[arg(long, default_value_t = true)] incremental: bool,
    #[arg(long, default_value_t = false)] replace: bool,
}

#[derive(Deserialize)] struct HeightResp { height: u64 }

#[tokio::main]
async fn main() -> Result<()> {
    let opts = Opts::parse();
    let client = reqwest::Client::new();
    let HeightResp { height: target_h } = client.get(format!("{}/debug/state/height", opts.dst)).send().await?.json().await?;
    let items: Vec<AccountState> = if opts.incremental && target_h > 0 {
        client.get(format!("{}/debug/state/diff/{}", opts.src, target_h)).send().await?.json().await?
    } else {
        client.get(format!("{}/debug/state/snapshot", opts.src)).send().await?.json().await?
    };
    #[derive(Serialize)] struct RestoreReq { replace: bool, items: Vec<AccountState> }
    let body = RestoreReq { replace: opts.replace, items };
    let resp = client.post(format!("{}/debug/state/restore", opts.dst)).json(&body).send().await?;
    println!("state-sync -> target status: {}", resp.status());
    Ok(())
}
