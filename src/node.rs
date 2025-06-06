use crate::error::AlpenGlowResult;

pub trait AlpenGlowNode {
    type Pubkey;
    type Signature;
    type Message;
    fn pubkey(&self) -> Self::Pubkey;
    fn sign_message(&self, message: &[u8]) -> Self::Signature;
    fn verify_sig(sig: Self::Signature, pubkey: Self::Pubkey, message: &[u8]) -> bool;
    fn is_leader(&self) -> bool;
    fn stake(&self) -> u64;
    fn send_messages(&self, msgs: Vec<Self::Message>) -> AlpenGlowResult<()>;
}

pub mod solana_alpenglow_node {
    use crossbeam::channel;
    use solana_pubkey::Pubkey;
    use solana_signature::Signature;
    use solana_signer::{EncodableKey, Signer};
    use tracing::{error, info};

    use crate::{
        config::AlpenGlowConfig,
        error::{AlpenGlowError, AlpenGlowResult},
        message::solana_alpenglow_message::SolanaMessage,
        node::AlpenGlowNode,
    };
    use solana_keypair::Keypair;
    use std::fmt::{Debug, Display};
    pub struct SolanaNode {
        keypair: Keypair,
        stake: u64,
        message_sender: channel::Sender<Vec<SolanaMessage>>,
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
        pub fn init(
            config: &AlpenGlowConfig,
        ) -> AlpenGlowResult<(Self, channel::Receiver<Vec<SolanaMessage>>)> {
            let (node, rx) = Self::init_inner(config)?;
            node.log();
            Ok((node, rx))
        }

        pub fn init_inner(
            config: &AlpenGlowConfig,
        ) -> AlpenGlowResult<(Self, channel::Receiver<Vec<SolanaMessage>>)> {
            let (tx, rx) = crossbeam::channel::unbounded();
            Ok((
                Self {
                    keypair: Keypair::read_from_file(config.ed25519_key_path())
                        .map_err(|_| AlpenGlowError::InvalidKeypair)?,
                    stake: 0,
                    message_sender: tx,
                },
                rx,
            ))
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
        type Message = SolanaMessage;
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

        fn send_messages(&self, msgs: Vec<SolanaMessage>) -> AlpenGlowResult<()> {
            let msgs_len = msgs.len();

            match self.message_sender.send(msgs) {
                Ok(_) => {
                    info!("sent {} messages", msgs_len);
                }
                Err(e) => error!("Error sending messages {}", e),
            }

            Ok(())
        }

        fn stake(&self) -> u64 {
            self.stake
        }
    }
}
