use crate::SubmitApi;

#[derive(Clone)]
pub struct NodeApiAdapter(pub std::sync::Arc<dyn SubmitApi>);

#[async_trait::async_trait]
impl rpc::NodeApi for NodeApiAdapter {
    async fn submit_transfer(&self, t: rpc::TransferReq) -> anyhow::Result<types::Receipt> {
        let tx = types::Transfer {
            from: t.from,
            to: t.to,
            amount: t.amount,
            nonce: 0,
            payload: None,
        };
        self.0.submit_transfer(tx).await
    }

    async fn get_balance(&self, addr: String) -> anyhow::Result<u64> {
        self.0.get_balance(addr).await
    }
}
