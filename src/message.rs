use std::{
    fmt::Debug,
    mem,
    net::SocketAddrV4,
    sync::{Arc, RwLock},
    thread::{self, JoinHandle},
};

use crossbeam::channel::Receiver;
use tracing::{error, info};

use crate::error::{AlpenGlowError, AlpenGlowResult};

pub trait AlpenGlowMessage: Debug + Sized + Sync + Send {
    const MESSAGE_DATA_START: usize;
    const MESSAGE_LEN_START: usize;
    const MESSAGE_TYPE_START: usize;
    fn sender(&self) -> &SocketAddrV4;
    fn receiver(&self) -> &SocketAddrV4;
    fn message_type(&self) -> MessageType;
    fn message_len(&self) -> u16;
    fn pack(&self) -> Vec<u8>;
    fn unpack(bytes: &[u8]) -> AlpenGlowResult<Self>;
    fn unpack_batch(bytes: &[u8]) -> AlpenGlowResult<Vec<Self>>;
    fn data(&self) -> Vec<u8>;
}

pub type MessageLen = u16;

pub type Hash = [u8; 64];

pub type Slot = u64;

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

pub fn pack_socket_add_v4(addr: &SocketAddrV4) -> Vec<u8> {
    let ip = addr.ip().octets();
    let port: [u8; 2] = addr.port().to_le_bytes().try_into().expect("err ip");

    let mut buffer = Vec::new();

    buffer.extend_from_slice(&ip);
    buffer.extend_from_slice(&port);

    buffer
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
                match T::unpack_batch(m.as_slice()) {
                    Ok(m) => {
                        if let Ok(mut mp) = message_pool.write() {
                            let messages_len = m.len();
                            match mp.store_batch(m) {
                                Ok(()) => {
                                    info!("alpenglowMessagePool : push msgs {:?}", messages_len);
                                }
                                Err(e) => {
                                    error!("alpenglowMessagePool : error({}) pushing msgs", e)
                                }
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
        net::{Ipv4Addr, SocketAddrV4},
        sync::{Arc, RwLock},
    };

    use solana_pubkey::Pubkey;
    use tracing::{error, info, warn};

    use crate::{
        error::{AlpenGlowError, AlpenGlowResult},
        message::{
            AlpenGlowMessage, AlpenGlowMessagePool, Hash, MessageLen, MessageType, Slot,
            pack_socket_add_v4,
        },
        network::MAX_QUIC_MESSAGE_BYTES,
    };

    pub struct SolanaMessage {
        sender: SocketAddrV4,
        receiver: SocketAddrV4,
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
        pub fn build(
            data: Vec<u8>,
            msg_type: MessageType,
            sender: SocketAddrV4,
            receiver: SocketAddrV4,
        ) -> SolanaMessage {
            Self {
                message_len: data.len() as u16,
                data,
                message_type: msg_type,
                sender,
                receiver,
            }
        }
    }

    impl AlpenGlowMessage for SolanaMessage {
        const MESSAGE_DATA_START: usize = 15;
        const MESSAGE_TYPE_START: usize = 14;
        const MESSAGE_LEN_START: usize = 12;

        fn sender(&self) -> &SocketAddrV4 {
            &self.sender
        }

        fn receiver(&self) -> &SocketAddrV4 {
            &self.receiver
        }

        fn pack(&self) -> Vec<u8> {
            let len_bytes: [u8; 2] = self.message_len.to_le_bytes();
            let mut buffer = Vec::with_capacity(
                mem::size_of::<SocketAddrV4>()
                    + mem::size_of::<MessageLen>()
                    + MessageType::size()
                    + self.message_len as usize,
            );

            buffer.extend_from_slice(&pack_socket_add_v4(&self.sender));

            buffer.extend_from_slice(&pack_socket_add_v4(&self.receiver));

            buffer.extend_from_slice(&len_bytes);

            buffer.push(self.message_type as u8);

            buffer.extend_from_slice(self.data.as_slice());

            buffer
        }

        fn unpack(bytes: &[u8]) -> AlpenGlowResult<Self> {
            if bytes.len() as u64 > MAX_QUIC_MESSAGE_BYTES {
                return Err(AlpenGlowError::InvalidMessage);
            }

            let sender = {
                let ip = (bytes[0], bytes[1], bytes[2], bytes[3]);
                let port = u16::from_le_bytes(
                    bytes[4..6]
                        .try_into()
                        .map_err(|_| AlpenGlowError::InvalidMessage)?,
                );

                SocketAddrV4::new(Ipv4Addr::new(ip.0, ip.1, ip.2, ip.3), port)
            };

            let receiver = {
                let ip = (bytes[6], bytes[7], bytes[8], bytes[9]);
                let port = u16::from_le_bytes(
                    bytes[10..12]
                        .try_into()
                        .map_err(|_| AlpenGlowError::InvalidMessage)?,
                );

                SocketAddrV4::new(Ipv4Addr::new(ip.0, ip.1, ip.2, ip.3), port)
            };

            let message_len = MessageLen::from_le_bytes(
                bytes[Self::MESSAGE_LEN_START..Self::MESSAGE_LEN_START + 2]
                    .try_into()
                    .map_err(|_| AlpenGlowError::InvalidMessage)?,
            );

            let message_type = MessageType::try_from(bytes[Self::MESSAGE_TYPE_START])?;

            Ok(SolanaMessage {
                sender,
                receiver,
                message_type,
                message_len,
                data: bytes
                    [Self::MESSAGE_DATA_START..(Self::MESSAGE_DATA_START + message_len as usize)]
                    .to_vec(),
            })
        }

        fn unpack_batch(bytes: &[u8]) -> AlpenGlowResult<Vec<Self>> {
            if bytes.len() as u64 > MAX_QUIC_MESSAGE_BYTES {
                return Err(AlpenGlowError::InvalidMessage);
            }

            let mut messages = Vec::new();

            let end = bytes.len();
            let mut cur = 0; // Pubkey offset

            while cur < end {
                let message_len = MessageLen::from_le_bytes(
                    bytes[cur + Self::MESSAGE_LEN_START..(cur + Self::MESSAGE_TYPE_START)]
                        .try_into()
                        .map_err(|_| AlpenGlowError::InvalidMessage)?,
                );

                println!("message len {}", message_len);

                let read_end_index = cur + 32 + 2 + 1 + message_len as usize;

                match Self::unpack(&bytes[cur..read_end_index]) {
                    Ok(m) => {
                        messages.push(m);
                    }
                    Err(e) => error!("error unpacking {}", e),
                }

                cur = read_end_index;
            }

            Ok(messages)
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
        pub voter_address: Pubkey,
        pub vote: bool,
        pub block: Hash,
        pub slot: Slot,
    }

    impl VoteMessage {
        const LEN: u16 = 32 + 1 + 64 + 8;
        pub unsafe fn unpack(bytes: &[u8]) -> AlpenGlowResult<VoteMessage> {
            if bytes.len() as u16 != Self::LEN {
                return Err(AlpenGlowError::InvalidMessage);
            }
            let voter_address = Pubkey::new_from_array(
                bytes[0..32]
                    .try_into()
                    .map_err(|_| AlpenGlowError::InvalidMessage)?,
            );
            let vote = bytes[32] > 0;
            let block: Hash = bytes[33..97]
                .try_into()
                .map_err(|_| AlpenGlowError::InvalidMessage)?;

            let slot: Slot = u64::from_le_bytes(
                bytes[97..105]
                    .try_into()
                    .map_err(|_| AlpenGlowError::InvalidMessage)?,
            );

            let vote_msg = Self {
                vote,
                slot,
                block,
                voter_address,
            };
            Ok(vote_msg)
        }

        pub fn pack(&self) -> Vec<u8> {
            let mut buf = Vec::new();

            buf.extend_from_slice(self.voter_address.as_array());

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
                "VoteMessage(voter {}, vote : {} , block_hash : {}, slot:  {}",
                self.voter_address,
                self.vote,
                bs58::encode(self.block.as_slice()).into_string(),
                self.slot
            )
        }
    }

    #[derive(Clone, Debug)]
    pub struct PingMessage {
        pub node_address: Pubkey,
        pub message: Vec<u8>,
    }

    impl PingMessage {
        const LEN: u16 = 32 + 32;
        pub unsafe fn unpack(bytes: &[u8]) -> AlpenGlowResult<PingMessage> {
            if bytes.len() as u16 != Self::LEN {
                return Err(AlpenGlowError::InvalidMessage);
            }
            let node_address = Pubkey::new_from_array(
                bytes[0..32]
                    .try_into()
                    .map_err(|_| AlpenGlowError::InvalidMessage)?,
            );

            let message = bytes[32..64].to_vec();

            let ping_msg = Self {
                node_address,
                message,
            };
            Ok(ping_msg)
        }

        pub fn pack(&self) -> Vec<u8> {
            let mut buf = Vec::new();

            buf.extend_from_slice(self.node_address.as_array());

            buf.extend_from_slice(self.message.as_slice());

            buf
        }

        pub fn from_solana_message(msg: &SolanaMessage) -> AlpenGlowResult<PingMessage> {
            unsafe { PingMessage::unpack(&msg.data) }
        }

        pub fn log(&self) {
            info!(
                "PingMessage(node {}, message : {}",
                self.node_address.to_string(),
                String::from_utf8_lossy(&self.message).to_string(),
            )
        }
    }

    pub struct SolanaMessagePool {
        pub vote_messages: Vec<VoteMessage>,
        pub ping_messages: Vec<PingMessage>,
    }

    impl SolanaMessagePool {
        pub fn init() -> Arc<RwLock<Self>> {
            Arc::new(RwLock::new(Self {
                vote_messages: Vec::new(),
                ping_messages: Vec::new(),
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
                match m.message_type {
                    MessageType::Vote => match VoteMessage::from_solana_message(&m) {
                        Ok(m) => {
                            m.log();
                            self.vote_messages.push(m);
                        }
                        Err(e) => println!("err {:?}", e),
                    },
                    MessageType::Ping => match PingMessage::from_solana_message(&m) {
                        Ok(m) => {
                            m.log();
                            self.ping_messages.push(m);
                        }
                        Err(e) => println!("err {:?}", e),
                    },
                    _ => warn!("message type not implemented"),
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
