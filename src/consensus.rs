use std::{
    collections::HashMap,
    net::SocketAddr,
    str::FromStr,
    sync::{Arc, RwLock},
};

use tracing::info;

use crate::{config::AlpenGlowConfig, message::AlpenGlowMessage};

pub type Stake = u64;

pub struct Peer {
    ip: SocketAddr,
    stake: Stake,
}

pub type PeerPoolSync<M> = Arc<RwLock<PeerPool<M>>>;

pub struct PeerPool<M: AlpenGlowMessage> {
    peers: HashMap<M::Address, Peer>,
}

impl<M: AlpenGlowMessage> PeerPool<M> {
    pub fn init(config: &AlpenGlowConfig) -> Arc<RwLock<Self>> {
        let mut peer_map = HashMap::new();

        let peers = config.get_peers();

        peers.into_iter().for_each(|p| {
            peer_map.insert(
                M::Address::from_str(&p.0).unwrap_or(M::Address::default()),
                Peer {
                    ip: SocketAddr::from_str(&p.1).expect("invalid socket address"),
                    stake: p.2,
                },
            );
        });

        Arc::new(RwLock::new(Self { peers: peer_map }))
    }

    pub fn get_peer_ip(&self, peer: &M::Address) -> Option<SocketAddr> {
        self.peers.get(peer).map(|p| p.ip)
    }

    pub fn get_peer_stake(&self, peer: &M::Address) -> Option<Stake> {
        self.peers.get(peer).map(|p| p.stake)
    }

    pub fn check_peer_exists(&self, peer: &M::Address) -> bool {
        self.peers.contains_key(peer)
    }

    pub fn upsert_peer(&mut self, peer_pubkey: M::Address, peer: Peer) {
        self.peers.insert(peer_pubkey, peer);
    }

    pub fn get_random_peer(&self) -> Option<String> {
        self.peers.keys().next().map(|k| k.to_string())
    }

    pub fn log(&self) {
        for i in &self.peers {
            info!(
                "Peers(Node {}, Ip {}, Stake {})",
                i.0.to_string(),
                i.1.ip,
                i.1.stake
            );
        }
    }
}
