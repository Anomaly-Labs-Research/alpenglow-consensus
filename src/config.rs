use serde::Deserialize;
use std::{fs, net::SocketAddr, str::FromStr, sync::Arc};

use crate::consensus::Stake;

#[derive(Debug, Deserialize)]
struct ServerConfigToml {
    port: u16,
    host: String,
}

#[derive(Debug, Deserialize)]
struct ClientConfigToml {
    port: u16,
    host: String,
}

#[derive(Debug, Deserialize)]
struct KeysConfig {
    rsa_cert_path: String,
    rsa_key_path: String,
    ed25519_key_path: String,
}

#[derive(Debug, Deserialize)]
struct Peers {
    peers: Vec<(String, String, Stake)>,
}

#[derive(Debug, Deserialize)]
pub struct AlpenGlowConfig {
    server: ServerConfigToml,
    client: ClientConfigToml,
    keys: KeysConfig,
    peers: Peers,
}

impl AlpenGlowConfig {
    pub fn parse() -> Arc<Self> {
        let content = fs::read_to_string("Config.toml").expect("Could not read file");
        Arc::new(toml::from_str(&content).expect("Invalid TOML format"))
    }

    pub fn server_port(&self) -> u16 {
        self.server.port
    }

    pub fn server_addr(&self) -> String {
        self.server.host.to_string()
    }

    pub fn server_addr_with_port(&self) -> String {
        format!("{}:{}", self.server_addr(), self.server_port())
    }

    pub fn server_socket_addr(&self) -> SocketAddr {
        SocketAddr::from_str(&self.server_addr_with_port()).expect("invalid socket address")
    }

    pub fn client_port(&self) -> u16 {
        self.client.port
    }

    pub fn client_addr(&self) -> String {
        self.client.host.to_string()
    }

    pub fn client_socket_addr(&self) -> SocketAddr {
        SocketAddr::from_str(&self.client_addr_with_port()).expect("invalid socket address")
    }

    pub fn client_addr_with_port(&self) -> String {
        format!("{}:{}", self.client_addr(), self.client_port())
    }

    pub fn rsa_cert_path(&self) -> String {
        self.keys.rsa_cert_path.clone()
    }

    pub fn rsa_key_path(&self) -> String {
        self.keys.rsa_key_path.clone()
    }

    pub fn ed25519_key_path(&self) -> String {
        self.keys.ed25519_key_path.clone()
    }

    pub fn get_peers(&self) -> Vec<(String, String, Stake)> {
        self.peers.peers.clone()
    }
}
