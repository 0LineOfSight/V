
use clap::Parser;
use serde::Serialize;
use std::time::{Instant, Duration};
use tokio::sync::Semaphore;
use std::sync::Arc;
use anyhow::Result;

#[derive(Parser, Debug, Clone)]
#[command(name="bench", about="Simple load generator for the node RPC")]
struct Opts {
    #[arg(long, default_value = "http://127.0.0.1:8080")] url: String,
    #[arg(long, default_value_t = 1000)] n: usize,
    #[arg(long, default_value_t = 32)] concurrency: usize,
    #[arg(long, default_value = "alice")] from: String,
    #[arg(long, default_value = "bench-bob")] to: String,
    #[arg(long)] csv: Option<String>,
}
#[derive(Serialize)] struct TransferReq<'a> { from: &'a str, to: &'a str, amount: u64, nonce: u64 }

#[tokio::main]
async fn main() -> Result<()> {
    let opts = Opts::parse(); let client = reqwest::Client::new(); let sem = Arc::new(Semaphore::new(opts.concurrency));
    let submit_url = format!("{}/v1/tx/transfer", opts.url);
    let start = Instant::now(); let mut handles = Vec::with_capacity(opts.n);
    for i in 0..opts.n {
        let permit = sem.clone().acquire_owned().await.unwrap();
        let client = client.clone(); let submit_url = submit_url.clone(); let from = opts.from.clone(); let to = opts.to.clone();
        handles.push(tokio::spawn(async move {
            let _p = permit; let t0 = Instant::now();
            let req = TransferReq { from: &from, to: &to, amount: 1, nonce: i as u64 + 1 };
            let _resp = client.post(&submit_url).json(&req).send().await; Ok::<Duration, reqwest::Error>(t0.elapsed())
        }));
    }
    let mut latencies: Vec<Duration> = Vec::with_capacity(opts.n);
    for h in handles { latencies.push(h.await.unwrap().unwrap()); }
    let elapsed = start.elapsed(); latencies.sort();
    let p50 = pct(&latencies, 0.50); let p95 = pct(&latencies, 0.95); let p99 = pct(&latencies, 0.99);
    let tps = (opts.n as f64) / elapsed.as_secs_f64();
    println!("n={}, concurrency={}, elapsed={:.2}s, tps={:.1}", opts.n, opts.concurrency, elapsed.as_secs_f64(), tps);
    println!("p50={:.1} ms  p95={:.1} ms  p99={:.1} ms", ms(p50), ms(p95), ms(p99));
    if let Some(path) = &opts.csv { let mut wtr = csv::Writer::from_path(path)?; wtr.write_record(&["latency_ms"])?; for d in &latencies { wtr.write_record(&[format!("{}", ms(*d))])?; } wtr.flush()?; println!("Wrote {}", path); }
    Ok(())
}
fn pct(v: &Vec<Duration>, q: f64) -> Duration { if v.is_empty() { return Duration::from_millis(0); } let idx = ((v.len() as f64 - 1.0) * q).round() as usize; v[idx] }
fn ms(d: Duration) -> u128 { (d.as_secs_f64() * 1000.0) as u128 }
