use quinn::{Endpoint, ServerConfig, ClientConfig, TransportConfig};
use quinn::crypto::rustls::QuicClientConfig;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, warn};

// Rustls 0.23
use rustls::client::danger::{ServerCertVerified, ServerCertVerifier, HandshakeSignatureValid};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::{ClientConfig as RustlsClientConfig, DigitallySignedStruct, SignatureScheme};

#[derive(Debug, Clone)]
pub struct NetOut { pub addr: SocketAddr, pub data: Vec<u8> }

#[derive(Debug, Clone)]
pub enum QuicEvent {
    Received { remote: SocketAddr, data: Vec<u8> },
    Connected { remote: SocketAddr },
    Closed { remote: SocketAddr },
}

#[derive(Clone)]
pub struct QuicHandle { pub outbound: mpsc::Sender<NetOut> }

pub async fn spawn_quic_server(bind: &str) -> anyhow::Result<(QuicHandle, mpsc::Receiver<QuicEvent>)> {
    let bind_addr: SocketAddr = bind.parse()?;

    let cert = generate_self_signed()?;
    let mut server_config = ServerConfig::with_single_cert(vec![cert.cert.clone()], cert.key)?;
    let mut transport = TransportConfig::default();
    transport.keep_alive_interval(Some(std::time::Duration::from_secs(5)));
    server_config.transport_config(Arc::new(transport));

    let endpoint = Endpoint::server(server_config, bind_addr)?;
    info!(%bind_addr, "QUIC server listening");

    let (tx_outbound, mut rx_outbound) = mpsc::channel::<NetOut>(4096);
    let (tx_events, rx_events) = mpsc::channel::<QuicEvent>(4096);

    // Accept loop
    {
        let ep = endpoint.clone();
        let tx_events_accept = tx_events.clone();
        tokio::spawn(async move {
            loop {
                match ep.accept().await {
                    Some(connecting) => match connecting.await {
                        Ok(conn) => {
                            let remote = conn.remote_address();
                            let _ = tx_events_accept.send(QuicEvent::Connected { remote }).await;
                            let tx_stream = tx_events_accept.clone();
                            tokio::spawn(async move {
                                loop {
                                    match conn.accept_bi().await {
                                        Ok((mut send, mut recv)) => {
                                            while let Ok(Some(chunk)) = recv.read_chunk(usize::MAX, true).await {
                                                let data = chunk.bytes.to_vec();
                                                let _ = tx_stream.send(QuicEvent::Received { remote, data }).await;
                                            }
                                            let _ = send.finish();
                                        }
                                        Err(_) => {
                                            let _ = tx_stream.send(QuicEvent::Closed { remote }).await;
                                            break;
                                        }
                                    }
                                }
                            });
                        }
                        Err(e) => warn!("QUIC accept (handshake) error: {e}"),
                    },
                    None => break,
                }
            }
        });
    }

    let client_endpoint = endpoint.clone();
    let pool = Arc::new(ConnPool { endpoint: client_endpoint, conns: tokio::sync::Mutex::new(std::collections::HashMap::new()) });

    tokio::spawn({
        let pool = pool.clone();
        async move {
            while let Some(NetOut { addr, data }) = rx_outbound.recv().await {
                if let Err(e) = pool.send(addr, data).await {
                    warn!("QUIC send error: {e}");
                    pool.drop_conn(addr).await;
                }
            }
        }
    });

    Ok((QuicHandle { outbound: tx_outbound }, rx_events))
}

struct ConnPool {
    endpoint: quinn::Endpoint,
    conns: tokio::sync::Mutex<std::collections::HashMap<SocketAddr, quinn::Connection>>,
}
impl ConnPool {
    async fn get(&self, addr: SocketAddr) -> anyhow::Result<quinn::Connection> {
        if let Some(c) = self.conns.lock().await.get(&addr).cloned() { return Ok(c); }

        let noverify = Arc::new(NoCertificateVerification {});
        let rustls_client = RustlsClientConfig::builder().dangerous().with_custom_certificate_verifier(noverify).with_no_client_auth();
        let quic_crypto = QuicClientConfig::try_from(rustls_client)?;
        let client_config = ClientConfig::new(Arc::new(quic_crypto));
        let connecting = self.endpoint.connect_with(client_config, addr, "localhost")?;
        let conn = connecting.await?;
        self.conns.lock().await.insert(addr, conn.clone());
        Ok(conn)
    }
    async fn drop_conn(&self, addr: SocketAddr) { self.conns.lock().await.remove(&addr); }
    async fn send(&self, addr: SocketAddr, data: Vec<u8>) -> anyhow::Result<()> {
        let conn = self.get(addr).await?;
        let (mut send, _recv) = conn.open_bi().await?;
        send.write_all(&data).await?;
        send.finish()?;
        Ok(())
    }
}

#[derive(Debug)]
struct NoCertificateVerification {}
impl ServerCertVerifier for NoCertificateVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> { Ok(ServerCertVerified::assertion()) }
    fn verify_tls12_signature(&self, _m: &[u8], _c: &CertificateDer<'_>, _d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> { Ok(HandshakeSignatureValid::assertion()) }
    fn verify_tls13_signature(&self, _m: &[u8], _c: &CertificateDer<'_>, _d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> { Ok(HandshakeSignatureValid::assertion()) }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::ED25519,
        ]
    }
}

struct CertPair { cert: CertificateDer<'static>, key: PrivateKeyDer<'static> }
fn generate_self_signed() -> anyhow::Result<CertPair> {
    let ck = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])?;
    let cert_der: CertificateDer<'static> = ck.cert.der().clone();
    let key_der: PrivateKeyDer<'static> = PrivateKeyDer::Pkcs8(ck.signing_key.serialize_der().into());
    Ok(CertPair { cert: cert_der, key: key_der })
}
