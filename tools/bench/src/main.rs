use clap::Parser;
use serde::{Serialize, Deserialize};
use std::time::Instant;
use tokio::sync::{Semaphore, mpsc};
use anyhow::Result;

use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug, Clone)]
#[command(about="Simple load generator for the node RPC (POST /transfer)")]
struct Opts {
    /// Base URL to the node RPC (e.g. http://127.0.0.1:8367)
    #[arg(long, default_value = "http://127.0.0.1:8080")]
    url: String,

    /// Total number of requests
    #[arg(long, default_value_t = 1000)]
    n: usize,

    /// Concurrency (in-flight requests)
    #[arg(long, default_value_t = 32)]
    concurrency: usize,

    /// Logical sender
    #[arg(long, default_value = "alice")]
    from: String,

    /// Logical recipient
    #[arg(long, default_value = "bench-bob")]
    to: String,

    /// Optional CSV file name to write per-request latencies (ms)
    #[arg(long)]
    csv: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TransferReq {
    from: String,
    to: String,
    amount: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag="status")]
enum Status {
    Committed,
    Rejected(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Receipt {
    tx_id: String,
    status: Status,
    block_height: u64,
    latency_ms: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_target(false)
        .init();

    let opt = Opts::parse();
    let base = opt.url.trim_end_matches('/').to_string();
    let url = format!("{}/transfer", base);

    let client = reqwest::Client::builder()
        
        .build()?;

    let sem = std::sync::Arc::new(Semaphore::new(opt.concurrency));
    let (tx, mut rx) = mpsc::channel::<f64>(opt.n);
    let start = Instant::now();

    for _ in 0..opt.n {
        let permit = sem.clone().acquire_owned().await?;
        let client = client.clone();
        let url = url.clone();
        let from = opt.from.clone();
        let to = opt.to.clone();
        let tx = tx.clone();

        tokio::spawn(async move {
            let _permit = permit;
            let t0 = Instant::now();
            let body = TransferReq { from, to, amount: 1 };
            let resp = client.post(&url).json(&body).send().await;
            let dt_ms = t0.elapsed().as_secs_f64() * 1000.0;

            if let Ok(r) = resp {
                if r.status().is_success() {
                    // Consume body to keep pressure realistic
                    let _ = r.bytes().await;
                }
            }
            let _ = tx.send(dt_ms).await;
        });
    }
    drop(tx); // close the channel when all tasks spawned

    let mut lats_ms = Vec::with_capacity(opt.n);
    while let Some(v) = rx.recv().await {
        lats_ms.push(v);
    }

    let elapsed = start.elapsed().as_secs_f64();
    let tps = (opt.n as f64) / elapsed;
    lats_ms.sort_by(|a,b| a.partial_cmp(b).unwrap());
    let p = |q: f64| -> f64 {
        if lats_ms.is_empty() { return 0.0; }
        let idx = ((lats_ms.len() as f64 - 1.0) * q).round() as usize;
        lats_ms[idx]
    };
    let p50 = p(0.50);
    let p95 = p(0.95);
    let p99 = p(0.99);

    println!("n={}, concurrency={}, elapsed={:.2}s, tps={:.1}", opt.n, opt.concurrency, elapsed, tps);
    println!("p50={:.0} ms  p95={:.0} ms  p99={:.0} ms", p50, p95, p99);

    if let Some(csv) = opt.csv.as_ref() {
        use std::io::Write;
        let mut wtr = std::fs::File::create(csv)?;
        for v in lats_ms.iter() {
            writeln!(wtr, "{:.3}", v)?;
        }
        println!("Wrote {}", csv);
    }

    Ok(())
}
