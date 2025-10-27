use libp2p::{identity, PeerId, SwarmBuilder};
use libp2p::{gossipsub, kad, mdns, noise, tcp, yamux, Multiaddr};
use libp2p::kad::store::MemoryStore;
use libp2p::swarm::SwarmEvent;
use std::time::Duration;
use std::error::Error as StdError;
use tokio::sync::mpsc;
use futures_util::StreamExt;
use tracing::{info, warn};

use libp2p_swarm_derive::NetworkBehaviour;

#[derive(NetworkBehaviour)]
pub struct NodeBehaviour {
    pub gossipsub: gossipsub::Behaviour,
    pub mdns: mdns::tokio::Behaviour,
    pub kademlia: kad::Behaviour<MemoryStore>,
}

pub async fn build_swarm() -> anyhow::Result<libp2p::Swarm<NodeBehaviour>> {
    let local_key = identity::Keypair::generate_ed25519();

    let mut swarm = SwarmBuilder::with_existing_identity(local_key)
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_dns()?
        .with_behaviour(|key| {
            let pid = PeerId::from(key.public());

            let gossip_cfg = gossipsub::ConfigBuilder::default()
                .heartbeat_interval(Duration::from_secs(1))
                .validation_mode(gossipsub::ValidationMode::Strict)
                .build()
                .map_err(|e| -> Box<dyn StdError + Send + Sync> {
                    Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
                })?;

            let gs = gossipsub::Behaviour::new(
                gossipsub::MessageAuthenticity::Signed(key.clone()),
                gossip_cfg,
            )
            .map_err(|e| -> Box<dyn StdError + Send + Sync> {
                Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
            })?;

            let mdns_beh = mdns::tokio::Behaviour::new(mdns::Config::default(), pid)
                .map_err(|e| -> Box<dyn StdError + Send + Sync> { Box::new(e) })?;

            let store = MemoryStore::new(pid);
            let kad_beh = kad::Behaviour::new(pid, store);

            Ok::<NodeBehaviour, Box<dyn StdError + Send + Sync>>(NodeBehaviour {
                gossipsub: gs,
                mdns: mdns_beh,
                kademlia: kad_beh,
            })
        })?
        .build();

    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;
    Ok(swarm)
}

/// Handle exposed to the rest of the node for publishing.
#[derive(Clone)]
pub struct P2pHandle {
    pub publish: mpsc::Sender<Vec<u8>>,
}

/// Spawn P2P with the signature expected by node/src/main.rs.
/// Returns: (handle_with_publish, inbound_messages_receiver)
pub async fn spawn_p2p(bind_addr: &str, topic: &str, bootstrap: Vec<Multiaddr>)
    -> anyhow::Result<(P2pHandle, mpsc::Receiver<Vec<u8>>)> {

    let mut swarm = build_swarm().await?;

    // Bind TCP listen (QUIC not wired here)
    let listen_addr: Multiaddr = format!("{bind_addr}").parse()?;
    if let Err(e) = swarm.listen_on(listen_addr.clone()) {
        warn!("listen_on {listen_addr} failed: {e}");
    }

    // Subscribe to topic
    let ident_topic = gossipsub::IdentTopic::new(topic.to_string());
    if let Err(e) = swarm.behaviour_mut().gossipsub.subscribe(&ident_topic) {
        warn!("gossipsub subscribe failed: {e}");
    }

    // Dial bootstrap peers if provided
    for addr in bootstrap {
        if let Err(e) = swarm.dial(addr.clone()) {
            warn!("dial {addr} failed: {e}");
        }
    }

    // Channels
    let (tx_inbound, rx_inbound) = mpsc::channel::<Vec<u8>>(1024);
    let (tx_publish, mut rx_publish) = mpsc::channel::<Vec<u8>>(1024);
    let handle = P2pHandle { publish: tx_publish.clone() };

    // Drive the swarm
    tokio::spawn(async move {
        loop {
            tokio::select! {
                maybe_msg = rx_publish.recv() => {
                    if let Some(data) = maybe_msg {
                        let _ = swarm.behaviour_mut().gossipsub.publish(ident_topic.clone(), data);
                    } else {
                        // publisher dropped; keep listening for inbound
                    }
                }
                event = swarm.select_next_some() => {
                    if let SwarmEvent::Behaviour(ev) = event {
                        match ev {
                            NodeBehaviourEvent::Gossipsub(gossipsub::Event::Message { message, .. }) => {
                                let _ = tx_inbound.send(message.data).await;
                            }
                            _ => { /* ignore */ }
                        }
                    }
                }
            }
        }
    });

    info!("spawn_p2p started on {bind_addr}, topic={topic}");
    Ok((handle, rx_inbound))
}
