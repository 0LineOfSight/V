
use serde::{Serialize, Deserialize};
use tokio::fs;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    pub rpc_addr: String,
    pub quic_addr: String,
    pub p2p_listen: String,
    pub node_id: u32,
    pub validators: String,
    pub validators_keys: String,
    pub node_sk: Option<String>,
    pub db_path: String,
}

pub async fn load_yaml(path: &str) -> anyhow::Result<NodeConfig> {
    let data = fs::read_to_string(path).await?;
    let cfg: NodeConfig = serde_yaml::from_str(&data)?;
    Ok(cfg)
}

pub async fn watch_and_log(path: String) {
    let mut last = None;
    loop {
        let meta = fs::metadata(&path).await.ok();
        let changed = meta.as_ref().and_then(|m| m.modified().ok());
        if changed != last {
            last = changed;
            info!("config file changed or checked: {}", path);
        }
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    }
}
