use std::fmt::{Debug, Display};

use solana_keypair::Keypair;
use solana_pubkey::Pubkey;
use solana_signature::Signature;
use solana_signer::{EncodableKey, Signer};
use tracing::info;

use crate::{
    config::AlpenGlowConfig,
    error::{AlpenGlowError, AlpenGlowResult},
};

pub struct Node {
    keypair: Keypair,
    stake: u64,
}

impl Debug for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            format!(
                "Node:\n    Pubkey : {}\n    Stake : {}",
                self.pubkey(),
                self.stake()
            )
        )
    }
}

impl Display for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            format!(
                "Node:\n    Pubkey : {}\n    Stake : {}",
                self.pubkey(),
                self.stake()
            )
        )
    }
}

pub trait NodeIden {
    fn pubkey(&self) -> Pubkey;
    fn sign_message(&self, message: &[u8]) -> Signature;
    fn verify_sig(sig: Signature, pubkey: Pubkey, message: &[u8]) -> bool;
}

impl Node {
    pub fn init(config: &AlpenGlowConfig) -> AlpenGlowResult<Self> {
        Ok(Self {
            keypair: Keypair::read_from_file(config.ed25519_key_path())
                .map_err(|_| AlpenGlowError::InvalidKeypair)?,
            stake: 0,
        })
    }

    pub fn is_leader(&self) -> bool {
        todo!()
    }

    pub fn stake(&self) -> u64 {
        self.stake
    }

    pub fn vote(&self) {
        todo!()
    }

    pub fn log(&self) {
        info!(
            "Node {} initialized with stake {}",
            self.pubkey().to_string(),
            self.stake()
        )
    }
}

impl NodeIden for Node {
    fn pubkey(&self) -> Pubkey {
        self.keypair.pubkey()
    }

    fn sign_message(&self, message: &[u8]) -> Signature {
        self.keypair.sign_message(message)
    }

    fn verify_sig(sig: Signature, pubkey: Pubkey, message: &[u8]) -> bool {
        sig.verify(pubkey.as_array(), message)
    }
}
