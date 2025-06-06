pub trait AlpenGlowNode {
    type Pubkey;
    type Signature;
    fn pubkey(&self) -> Self::Pubkey;
    fn sign_message(&self, message: &[u8]) -> Self::Signature;
    fn verify_sig(sig: Self::Signature, pubkey: Self::Pubkey, message: &[u8]) -> bool;
    fn vote(&self);
    fn is_leader(&self) -> bool;
    fn stake(&self) -> u64;
}

pub mod solana_alpenglow_node {
    use solana_pubkey::Pubkey;
    use solana_signature::Signature;
    use solana_signer::{EncodableKey, Signer};
    use tracing::info;

    use crate::{
        config::AlpenGlowConfig,
        error::{AlpenGlowError, AlpenGlowResult},
        node::AlpenGlowNode,
    };
    use solana_keypair::Keypair;
    use std::fmt::{Debug, Display};
    pub struct SolanaNode {
        keypair: Keypair,
        stake: u64,
    }

    impl Debug for SolanaNode {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(
                f,
                "{}",
                format!(
                    "SolanaNode:\n    Pubkey : {}\n    Stake : {}",
                    self.pubkey(),
                    self.stake()
                )
            )
        }
    }

    impl Display for SolanaNode {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(
                f,
                "{}",
                format!(
                    "SolanaNode:\n    Pubkey : {}\n    Stake : {}",
                    self.pubkey(),
                    self.stake()
                )
            )
        }
    }

    impl SolanaNode {
        pub fn init(config: &AlpenGlowConfig) -> AlpenGlowResult<Self> {
            let node = Self::init_inner(config)?;
            node.log();
            Ok(node)
        }

        pub fn init_inner(config: &AlpenGlowConfig) -> AlpenGlowResult<Self> {
            Ok(Self {
                keypair: Keypair::read_from_file(config.ed25519_key_path())
                    .map_err(|_| AlpenGlowError::InvalidKeypair)?,
                stake: 0,
            })
        }

        pub fn is_leader(&self) -> bool {
            todo!()
        }

        pub fn log(&self) {
            info!(
                "SolanaNode {} initialized with stake {}",
                self.pubkey().to_string(),
                self.stake()
            )
        }
    }

    impl AlpenGlowNode for SolanaNode {
        type Pubkey = solana_pubkey::Pubkey;
        type Signature = solana_signature::Signature;
        fn pubkey(&self) -> Pubkey {
            self.keypair.pubkey()
        }

        fn sign_message(&self, message: &[u8]) -> Signature {
            self.keypair.sign_message(message)
        }

        fn verify_sig(sig: Signature, pubkey: Pubkey, message: &[u8]) -> bool {
            sig.verify(pubkey.as_array(), message)
        }
        fn is_leader(&self) -> bool {
            self.is_leader()
        }

        fn vote(&self) {}

        fn stake(&self) -> u64 {
            self.stake
        }
    }
}
