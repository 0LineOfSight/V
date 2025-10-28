pub use node::Node;

use std::sync::Arc;
use tokio::sync::mpsc;
use telemetry::init as telemetry_init;
use tracing::{info, warn};
use types::Receipt;
use once_cell::sync::Lazy;
use prometheus::{IntCounter, register_int_counter};
use net_quic::spawn_quic_server;
use net_p2p::spawn_p2p;
use libp2p::Multiaddr;
use configd::{load_yaml, watch_and_log};
use consensus::store::FileStore;

static EXEC_COMMITS: Lazy<IntCounter> = Lazy::new(|| register_int_counter!("exec_commits_total", "Committed batches at executor").unwrap());

#[derive(Clone, Debug)]
struct EnvConfig {
    rpc_addr: String, quic_addr: Option<String>, p2p_listen: Option<String>, p2p_bootstrap: Vec<Multiaddr>,
    node_id: u32, validators: String, validators_keys: String, node_sk: Option<String>,
    db_path: String, use_yaml: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    telemetry_init();
    info!("starting node (full hotfix 3)");

    let mut cfg = read_env_cfg();
    if let Some(path) = &cfg.use_yaml {
        let y = load_yaml(path).await?;
        cfg.rpc_addr = y.rpc_addr; cfg.quic_addr = Some(y.quic_addr); cfg.p2p_listen = Some(y.p2p_listen);
        cfg.node_id = y.node_id; cfg.validators = y.validators; cfg.db_path = y.db_path;
        cfg.validators_keys = y.validators_keys; cfg.node_sk = y.node_sk;
        tokio::spawn(watch_and_log(path.clone()));
    }

    run_node(cfg).await?;
    Ok(())
}

fn read_env_cfg() -> EnvConfig {
    let boots = std::env::var("P2P_BOOTSTRAP").unwrap_or_default();
    let p2p_bootstrap = boots.split(',').filter_map(|s| if s.trim().is_empty() { None } else { s.parse::<Multiaddr>().ok() }).collect::<Vec<_>>();
    EnvConfig {
        rpc_addr: std::env::var("RPC_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".to_string()),
        quic_addr: std::env::var("QUIC_ADDR").ok(),
        p2p_listen: std::env::var("P2P_LISTEN").ok(),
        p2p_bootstrap,
        node_id: std::env::var("NODE_ID").ok().and_then(|s| s.parse().ok()).unwrap_or(1),
        validators: std::env::var("VALIDATORS").unwrap_or_default(),
        validators_keys: std::env::var("VALIDATORS_KEYS").unwrap_or_default(),
        node_sk: std::env::var("NODE_SK").ok(),
        db_path: std::env::var("DB_PATH").unwrap_or_else(|_| "db".to_string()),
        use_yaml: std::env::var("CONFIG_YAML").ok(),
    }
}

async fn run_node(cfg: EnvConfig) -> anyhow::Result<()> {
    info!(?cfg, "launching node");

    let (to_consensus_tx, from_mempool_rx) = mpsc::channel(1024);
    let (to_exec_tx, mut from_consensus_rx) = mpsc::channel(1024);
    let (committed_tx, committed_rx) = mpsc::channel::<Receipt>(1024);

    let simple = Arc::new(exec::SimpleExecutor::new());
    let executor: Arc<dyn exec::Executor> = Arc::new(exec::BlockStmExecutor::new(simple));

    let (p2p_publish_opt, p2p_rx_opt) = if let Some(addr) = &cfg.p2p_listen {
        let (p2p_handle, mut p2p_in) = spawn_p2p(addr, "txs", cfg.p2p_bootstrap.clone()).await?;
        let (tx_to_mempool, rx_to_mempool) = mpsc::channel::<types::Tx>(4096);
        tokio::spawn(async move {
            while let Some(bytes) = p2p_in.recv().await {
                if let Ok(t) = serde_json::from_slice::<types::Transfer>(&bytes) {
                    let tx = types::Tx::new(t);
                    if let Err(_e) = tx_to_mempool.send(tx).await { break; }
                }
            }
        });
        (Some(p2p_handle.publish.clone()), Some(rx_to_mempool))
    } else { (None, None) };

    let (mempool_handle, mempool_rx) = mempool::spawn_mempool(25, 128, to_consensus_tx.clone());

    let node = crate::Node::new(mempool_handle.clone(), executor.clone(), p2p_publish_opt.clone());
    node.spawn_commit_listener(committed_rx);

    tokio::spawn(async move {
        mempool::run_mempool(mempool_rx, to_consensus_tx, 25, 128, p2p_rx_opt).await;
    });

    let store_dir = std::path::PathBuf::from("consensus_store");
    let qc_store = std::sync::Arc::new(FileStore::new(&store_dir));

    {
        use std::net::SocketAddr;
        use consensus::{Validators, Validator};
        let quic_addr = cfg.quic_addr.clone().unwrap_or_else(|| "127.0.0.1:7000".to_string());
        let (qhandle, qin) = spawn_quic_server(&quic_addr).await.expect("quic server");

        let mut id_to_pk: std::collections::HashMap<u32, crypto::PubKey> = std::collections::HashMap::new();
        for part in cfg.validators_keys.split(',').filter(|s| !s.trim().is_empty()) {
            if let Some((id_s, pk_hex)) = part.split_once('@') {
                if let Ok(id) = id_s.parse::<u32>() {
                    if let Ok(bytes) = hex::decode(pk_hex) { if bytes.len()==32 { let mut a=[0u8;32]; a.copy_from_slice(&bytes); id_to_pk.insert(id, crypto::PubKey(a)); } }
                }
            }
        }

        let mut nodes = Vec::new();
        for part in cfg.validators.split(',').filter(|s| !s.trim().is_empty()) {
            if let Some((id_s, addr_s)) = part.split_once('@') {
                if let (Ok(id), Ok(addr)) = (id_s.parse::<u32>(), addr_s.parse::<SocketAddr>()) {
                    let pk = id_to_pk.get(&id).cloned().unwrap_or_else(|| {
                        let (_sk, pk) = crypto::generate();
                        warn!("No ed25519 pubkey for id {}, using ephemeral {}", id, pk.hex()); pk
                    });
                    nodes.push(Validator { id, addr, pubkey: pk });
                }
            }
        }
        if nodes.is_empty() {
            let addr: SocketAddr = quic_addr.parse().expect("parse quic addr");
            let (_sk, pk) = crypto::generate();
            nodes.push(Validator { id: cfg.node_id, addr, pubkey: pk });
        }

        let my_sk = if let Some(hexsk) = cfg.node_sk.clone() {
            match hex::decode(hexsk) {
                Ok(bytes) if bytes.len() == 32 => { let mut a=[0u8;32]; a.copy_from_slice(&bytes); crypto::SecretKey(a) },
                _ => { let (sk,_pk)=crypto::generate(); warn!("NODE_SK invalid or missing; generated new {}", sk.hex()); sk }
            }
        } else { let (sk,_pk)=crypto::generate(); warn!("NODE_SK not provided; generated new {}", sk.hex()); sk };
        let my_pk = { let vk = ed25519_dalek::SigningKey::from_bytes(&my_sk.0).verifying_key(); crypto::PubKey(vk.to_bytes()) };

        let validators = Validators { self_id: cfg.node_id, nodes };
        let mut pk_map = std::collections::HashMap::new(); for v in &validators.nodes { pk_map.insert(v.id, v.pubkey.clone()); }
        let keys = consensus::KeySet { my_sk, my_pk, pks: pk_map };

        let to_exec_tx2 = to_exec_tx.clone();
        let qc_store_arc = qc_store.clone();
        tokio::spawn(async move {
            consensus::run_hotstuff_quic(from_mempool_rx, to_exec_tx2, 60, qhandle.outbound, qin, validators, keys, Some(qc_store_arc)).await;
        });
    }

    let exec2 = executor.clone();
    tokio::spawn(async move {
        while let Some((batch, height)) = from_consensus_rx.recv().await {
            let receipts = exec2.apply_batch_blocking(batch, height);
            EXEC_COMMITS.inc();
            for r in receipts {
                if let Err(e) = committed_tx.send(r).await { eprintln!("commit send error: {e}"); }
            }
        }
    });

    let api: Arc<dyn types::SubmitApi> = node.clone();
    rpc::serve(&cfg.rpc_addr, node::NodeApiAdapter(node.clone()), executor.clone()).await?;

    Ok(())
}
