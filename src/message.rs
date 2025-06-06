use std::{
    fmt::Debug,
    mem,
    sync::{Arc, RwLock},
    thread::{self, JoinHandle},
};

use crossbeam::channel::Receiver;
use tracing::{error, info};

use crate::{
    error::{AlpenGlowError, AlpenGlowResult},
    message::solana_alpenglow_message::VoteMessage,
};

pub trait AlpenGlowMessage: Debug + Sized + Sync + Send {
    const MAX_MESSAGE_LEN_BYTES: u16;
    const MESSAGE_DATA_START: usize;
    const MESSAGE_LEN_START: usize;
    fn message_type(&self) -> MessageType;
    fn message_len(&self) -> u16;
    fn pack(&self) -> Vec<u8>;
    fn unpack(bytes: &[u8]) -> AlpenGlowResult<Self>;
    fn data(&self) -> Vec<u8>;
}

pub type MessageLen = u16;

pub type Hash = [u8; 64];

pub type Slot = u16;

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum MessageType {
    Ping,
    DataShred,
    ACK,
    Vote,
}

impl MessageType {
    fn size() -> usize {
        mem::size_of::<MessageType>()
    }
}

impl TryFrom<u8> for MessageType {
    type Error = AlpenGlowError;
    fn try_from(value: u8) -> AlpenGlowResult<MessageType> {
        match value {
            0 => Ok(MessageType::Ping),
            1 => Ok(MessageType::DataShred),
            2 => Ok(MessageType::ACK),
            3 => Ok(MessageType::Vote),
            _ => Err(AlpenGlowError::InvalidMessage),
        }
    }
}

pub struct MessageProcesser;

impl MessageProcesser {
    pub fn spawn_with_receiver<T: AlpenGlowMessage>(
        rx: Receiver<Vec<u8>>,
        message_pool: Arc<RwLock<impl AlpenGlowMessagePool<Message = T> + 'static>>,
    ) -> JoinHandle<()> {
        info!("spawn : messagsProcesserThread");
        let message_pool = Arc::clone(&message_pool);
        thread::spawn(move || {
            while let Ok(m) = rx.recv() {
                match T::unpack(m.as_slice()) {
                    Ok(m) => {
                        if let Ok(mut mp) = message_pool.write() {
                            let msg_type = m.message_type();
                            let vote_msg = unsafe { VoteMessage::unpack(m.data().as_slice()) }.ok();
                            match mp.store(m) {
                                Ok(()) => {
                                    info!("alpenglowMessagePool : push msg {:?}", msg_type);
                                    if let Some(vote_msg) = vote_msg {
                                        vote_msg.log();
                                    }
                                }
                                Err(e) => error!(
                                    "alpenglowMessagePool : error({}) pushing msg {:?}",
                                    e, msg_type
                                ),
                            }
                        }
                    }
                    Err(e) => error!("alpenglowMessagePool : err unpacking msg {:?}", e),
                }
            }
        })
    }
}

pub trait AlpenGlowMessagePool: Send + Sync {
    type PoolMessage;
    type Message: AlpenGlowMessage;
    fn store(&mut self, msg: Self::Message) -> AlpenGlowResult<()>;
    fn store_batch(&mut self, msgs: Vec<Self::Message>) -> AlpenGlowResult<()>;
    fn get_msgs_by_type(&self, msg_type: MessageType) -> &[Self::PoolMessage];
    fn get_vote_messages_by_block_hash(&self, block_hash: Hash) -> Vec<&Self::PoolMessage>;
    fn get_vote_messages_by_slot(&self, slot: Slot) -> Vec<&Self::PoolMessage>;
}

pub mod solana_alpenglow_message {

    use std::{
        fmt::Debug,
        mem,
        sync::{Arc, RwLock},
    };

    use tracing::info;

    use crate::{
        error::{AlpenGlowError, AlpenGlowResult},
        message::{AlpenGlowMessage, AlpenGlowMessagePool, Hash, MessageLen, MessageType, Slot},
    };

    pub struct SolanaMessage {
        message_type: MessageType,
        message_len: u16,
        data: Vec<u8>,
    }

    impl Debug for SolanaMessage {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(
                f,
                "SolanaMessage(type {:?}, data {:?})",
                self.message_type,
                self.data.as_slice()
            )
        }
    }

    impl SolanaMessage {
        pub fn from_bytes_and_type(data: Vec<u8>, msg_type: MessageType) -> SolanaMessage {
            Self {
                message_len: data.len() as u16,
                data,
                message_type: msg_type,
            }
        }
    }

    impl AlpenGlowMessage for SolanaMessage {
        const MAX_MESSAGE_LEN_BYTES: u16 = 1024; // 1 MB

        const MESSAGE_DATA_START: usize = 3;
        const MESSAGE_LEN_START: usize = 1;

        fn pack(&self) -> Vec<u8> {
            let len_bytes: [u8; 2] = self.message_len.to_le_bytes();
            let mut buffer = Vec::with_capacity(
                MessageType::size() + mem::size_of::<MessageLen>() + self.message_len as usize,
            );

            buffer.extend_from_slice(&len_bytes);

            buffer.extend_from_slice(self.data.as_slice());

            buffer
        }

        fn unpack(bytes: &[u8]) -> AlpenGlowResult<Self> {
            if bytes.len() as u16 > Self::MAX_MESSAGE_LEN_BYTES {
                return Err(AlpenGlowError::InvalidMessage);
            }
            let message_type = MessageType::try_from(bytes[0])?;

            let message_len = MessageLen::from_le_bytes(
                bytes[Self::MESSAGE_LEN_START..=2]
                    .try_into()
                    .map_err(|_| AlpenGlowError::InvalidMessage)?,
            );

            Ok(SolanaMessage {
                message_type,
                message_len,
                data: bytes[Self::MESSAGE_DATA_START..].to_vec(),
            })
        }

        fn message_len(&self) -> u16 {
            self.message_len
        }

        fn message_type(&self) -> MessageType {
            self.message_type
        }

        fn data(&self) -> Vec<u8> {
            self.data.to_vec()
        }
    }

    #[derive(Clone, Debug)]
    pub struct VoteMessage {
        pub vote: bool,
        pub block: Hash,
        pub slot: Slot,
    }

    impl VoteMessage {
        const LEN: u16 = 1 + 64 + 2;
        pub unsafe fn unpack(bytes: &[u8]) -> AlpenGlowResult<VoteMessage> {
            if bytes.len() as u16 != Self::LEN {
                return Err(AlpenGlowError::InvalidMessage);
            }
            let vote = bytes[0] > 0;
            let block: Hash = bytes[1..65]
                .try_into()
                .map_err(|_| AlpenGlowError::InvalidMessage)?;

            let slot: Slot = u16::from_le_bytes(
                bytes[65..67]
                    .try_into()
                    .map_err(|_| AlpenGlowError::InvalidMessage)?,
            );

            Ok(Self { vote, slot, block })
        }

        pub fn pack(&self) -> Vec<u8> {
            let mut buf = Vec::new();

            buf.push(self.vote as u8);

            buf.extend_from_slice(self.block.as_slice());

            buf.extend_from_slice(self.slot.to_le_bytes().as_slice());

            buf
        }

        pub fn from_solana_message(msg: &SolanaMessage) -> AlpenGlowResult<VoteMessage> {
            unsafe { VoteMessage::unpack(&msg.data) }
        }

        pub fn log(&self) {
            info!(
                "VoteMesage(vote : {} , block_hash : {}",
                self.vote,
                bs58::encode(self.block.as_slice()).into_string()
            )
        }
    }

    pub struct SolanaMessagePool {
        pub vote_messages: Vec<VoteMessage>,
    }

    impl SolanaMessagePool {
        pub fn init() -> Arc<RwLock<Self>> {
            Arc::new(RwLock::new(Self {
                vote_messages: Vec::new(),
            }))
        }
    }

    impl AlpenGlowMessagePool for SolanaMessagePool {
        type PoolMessage = VoteMessage;
        type Message = SolanaMessage;

        fn store(&mut self, msg: SolanaMessage) -> AlpenGlowResult<()> {
            let vote_msg = VoteMessage::from_solana_message(&msg)?;
            self.vote_messages.push(vote_msg);
            Ok(())
        }

        fn store_batch(&mut self, msgs: Vec<SolanaMessage>) -> AlpenGlowResult<()> {
            for m in msgs {
                if let Ok(m) = VoteMessage::from_solana_message(&m) {
                    self.vote_messages.push(m);
                }
            }

            Ok(())
        }

        fn get_msgs_by_type(&self, msg_type: MessageType) -> &[VoteMessage] {
            match msg_type {
                MessageType::Vote => self.vote_messages.as_slice(),
                _ => panic!("not impl"),
            }
        }

        fn get_vote_messages_by_block_hash(&self, block_hash: Hash) -> Vec<&VoteMessage> {
            self.vote_messages
                .iter()
                .filter(|v| v.block == block_hash)
                .collect::<Vec<_>>()
        }

        fn get_vote_messages_by_slot(&self, slot: Slot) -> Vec<&VoteMessage> {
            self.vote_messages
                .iter()
                .filter(|v| v.slot == slot)
                .collect::<Vec<_>>()
        }
    }
}
