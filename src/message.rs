use std::{
    fmt::Debug,
    hash::Hash,
    mem,
    str::FromStr,
    sync::{Arc, RwLock},
    thread::{self, JoinHandle},
};

use crossbeam::channel::Receiver;
use tracing::{error, info};

use crate::{
    consensus::PeerPoolSync,
    error::{AlpenGlowError, AlpenGlowResult},
};

pub trait AlpenGlowMessage: Debug + Sized + Sync + Send {
    const MESSAGE_DATA_START: usize;
    const MESSAGE_LEN_START: usize;
    const MESSAGE_TYPE_START: usize;
    const MESSAGE_META_LENGTH: usize;
    type Address: Sized + Eq + Hash + Send + Sync + FromStr + Default + ToString;
    fn sender(&self) -> &Self::Address;
    fn receiver(&self) -> &Self::Address;
    fn message_type(&self) -> MessageType;
    fn message_len(&self) -> u16;
    fn pack(&self) -> Vec<u8>;
    fn unpack(bytes: &[u8]) -> AlpenGlowResult<Self>;
    fn unpack_batch(bytes: &[u8]) -> AlpenGlowResult<Vec<Self>>;
    fn data(&self) -> Vec<u8>;
}

pub type MessageLen = u16;

pub type IdenHash = [u8; 64];

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

pub struct MessageProcesser;

impl MessageProcesser {
    pub fn spawn_with_receiver<T: AlpenGlowMessage + 'static>(
        msg_receiver: &Receiver<Vec<u8>>,
        message_pool: AlpenGlowMessagePoolSync<T>,
        _peer_pool: &PeerPoolSync<T>,
    ) -> JoinHandle<()> {
        info!("spawn : messagsProcesserThread");
        let message_pool = Arc::clone(&message_pool);
        let msg_receiver = msg_receiver.clone();
        thread::spawn(move || {
            while let Ok(m) = msg_receiver.recv() {
                match T::unpack_batch(m.as_slice()) {
                    Ok(m) => {
                        if let Ok(mut mp) = message_pool.write() {
                            let messages_len = m.len();
                            match mp.process_batch(m) {
                                Ok(()) => {
                                    info!("alpenglowMessagePool : process msgs {:?}", messages_len);
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
    type Message: AlpenGlowMessage;
    fn process(&mut self, msg: Self::Message) -> AlpenGlowResult<()>;
    fn process_batch(&mut self, msgs: Vec<Self::Message>) -> AlpenGlowResult<()>;
}

pub type AlpenGlowMessagePoolSync<T> = Arc<RwLock<dyn AlpenGlowMessagePool<Message = T> + 'static>>;

pub mod solana_alpenglow_message {

    use std::{
        fmt::Debug,
        mem,
        sync::{Arc, RwLock},
    };

    use crossbeam::channel::Sender;
    use solana_pubkey::Pubkey;
    use tracing::{error, info, warn};

    use crate::{
        error::{AlpenGlowError, AlpenGlowResult},
        message::{AlpenGlowMessage, AlpenGlowMessagePool, MessageLen, MessageType, Slot},
        network::MAX_QUIC_MESSAGE_BYTES,
    };

    pub struct SolanaMessage {
        sender: Pubkey,
        receiver: Pubkey,
        message_len: u16,
        message_type: MessageType,
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
            sender: Pubkey,
            receiver: Pubkey,
        ) -> SolanaMessage {
            Self {
                sender,
                receiver,
                message_len: data.len() as u16,
                data,
                message_type: msg_type,
            }
        }
    }

    impl AlpenGlowMessage for SolanaMessage {
        const MESSAGE_DATA_START: usize = 67;
        const MESSAGE_TYPE_START: usize = 66;
        const MESSAGE_LEN_START: usize = 64;
        const MESSAGE_META_LENGTH: usize = 67;

        type Address = Pubkey;

        fn pack(&self) -> Vec<u8> {
            let len_bytes: [u8; 2] = self.message_len.to_le_bytes();

            let mut buffer = Vec::with_capacity(
                2 * mem::size_of::<Pubkey>()
                    + mem::size_of::<MessageLen>()
                    + MessageType::size()
                    + self.message_len as usize,
            );

            buffer.extend_from_slice(self.sender().as_array());

            buffer.extend_from_slice(self.receiver().as_array());

            buffer.extend_from_slice(&len_bytes);

            buffer.push(self.message_type as u8);

            buffer.extend_from_slice(self.data.as_slice());

            buffer
        }

        fn unpack(bytes: &[u8]) -> AlpenGlowResult<Self> {
            if bytes.len() as u64 > MAX_QUIC_MESSAGE_BYTES {
                return Err(AlpenGlowError::InvalidMessage);
            }

            let sender = Pubkey::new_from_array(
                bytes[0..32]
                    .try_into()
                    .map_err(|_| AlpenGlowError::InvalidMessage)?,
            );

            let receiver = Pubkey::new_from_array(
                bytes[32..64]
                    .try_into()
                    .map_err(|_| AlpenGlowError::InvalidMessage)?,
            );

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
            let mut cur = 0;

            while cur < end {
                let message_len = MessageLen::from_le_bytes(
                    bytes[cur + Self::MESSAGE_LEN_START..(cur + Self::MESSAGE_LEN_START + 2)]
                        .try_into()
                        .map_err(|_| AlpenGlowError::InvalidMessage)?,
                );

                let read_end_index = cur + Self::MESSAGE_META_LENGTH + message_len as usize;

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

        fn receiver(&self) -> &Pubkey {
            &self.receiver
        }

        fn sender(&self) -> &Pubkey {
            &self.sender
        }
    }

    #[derive(Clone, Debug)]
    pub struct VoteMessage {
        pub voter_address: Pubkey,
        pub vote: bool,
        pub block: [u8; 64],
        pub slot: Slot,
    }

    impl VoteMessage {
        const LEN: u16 = 32 + 1 + 64 + 8;
        pub fn unpack(bytes: &[u8]) -> AlpenGlowResult<VoteMessage> {
            if bytes.len() as u16 != Self::LEN {
                return Err(AlpenGlowError::InvalidMessage);
            }
            let voter_address = Pubkey::new_from_array(
                bytes[0..32]
                    .try_into()
                    .map_err(|_| AlpenGlowError::InvalidMessage)?,
            );
            let vote = bytes[32] > 0;
            let block: [u8; 64] = bytes[33..97]
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
            VoteMessage::unpack(&msg.data)
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
        pub src_node_address: Pubkey,
        pub message: Vec<u8>,
    }

    impl PingMessage {
        const MIN_LEN: u16 = 32;
        pub fn unpack(bytes: &[u8]) -> AlpenGlowResult<PingMessage> {
            if (bytes.len() as u16) < Self::MIN_LEN {
                return Err(AlpenGlowError::InvalidMessage);
            }
            let node_address = Pubkey::new_from_array(
                bytes[0..32]
                    .try_into()
                    .map_err(|_| AlpenGlowError::InvalidMessage)?,
            );

            let message = bytes[32..bytes.len()].to_vec();

            let ping_msg = Self {
                src_node_address: node_address,
                message,
            };
            Ok(ping_msg)
        }

        pub fn pack(&self) -> Vec<u8> {
            let mut buf = Vec::new();

            buf.extend_from_slice(self.src_node_address.as_array());

            buf.extend_from_slice(self.message.as_slice());

            buf
        }

        pub fn from_solana_message(msg: &SolanaMessage) -> AlpenGlowResult<PingMessage> {
            PingMessage::unpack(&msg.data)
        }

        pub fn log(&self) {
            info!(
                "PingMessage(src_node {}, message : {})",
                self.src_node_address.to_string(),
                String::from_utf8_lossy(&self.message).to_string(),
            )
        }
    }

    pub struct AckMessage {
        ack: bool,
    }

    impl AckMessage {
        const LEN: u16 = 1;
        pub fn unpack(bytes: &[u8]) -> AlpenGlowResult<AckMessage> {
            if (bytes.len() as u16) != Self::LEN {
                return Err(AlpenGlowError::InvalidMessage);
            }

            let ack = bytes[0] > 0;

            Ok(AckMessage { ack })
        }

        pub fn pack(&self) -> Vec<u8> {
            let buf = vec![self.ack as u8];
            buf
        }

        pub fn from_solana_message(msg: &SolanaMessage) -> AlpenGlowResult<AckMessage> {
            AckMessage::unpack(&msg.data)
        }

        pub fn log(&self, m: &SolanaMessage) {
            info!(
                "ACKMessage(from {}, ack : {} )",
                m.sender().to_string(),
                self.ack
            )
        }
    }

    pub struct SolanaMessagePool {
        node_address: Pubkey,
        msg_sender: Sender<Vec<SolanaMessage>>,
        pub vote_messages: Vec<VoteMessage>,
    }

    impl SolanaMessagePool {
        pub fn init(msg_sender: &Sender<Vec<SolanaMessage>>, node: Pubkey) -> Arc<RwLock<Self>> {
            Arc::new(RwLock::new(Self {
                node_address: node,
                vote_messages: Vec::new(),
                msg_sender: msg_sender.clone(),
            }))
        }

        pub fn node(&self) -> &Pubkey {
            &self.node_address
        }

        pub fn send_ack(&self, msg_sender: Pubkey, msg_receiver: Pubkey) {
            match self.msg_sender.send(vec![SolanaMessage::build(
                vec![(true as u8)],
                MessageType::ACK,
                msg_sender,
                msg_receiver,
            )]) {
                Ok(_) => {
                    info!("ACK msg sent to {}", msg_receiver.to_string());
                }
                Err(e) => {
                    error!(
                        "err sending ack msg to {} {}",
                        msg_sender.to_string(),
                        e.to_string()
                    );
                }
            }
        }

        pub fn msg_sender(&self) -> &Sender<Vec<SolanaMessage>> {
            &self.msg_sender
        }
    }

    impl AlpenGlowMessagePool for SolanaMessagePool {
        type Message = SolanaMessage;

        fn process(&mut self, msg: SolanaMessage) -> AlpenGlowResult<()> {
            let vote_msg = VoteMessage::from_solana_message(&msg)?;
            self.vote_messages.push(vote_msg);
            Ok(())
        }

        fn process_batch(&mut self, msgs: Vec<SolanaMessage>) -> AlpenGlowResult<()> {
            for m in msgs {
                match m.message_type {
                    MessageType::Vote => match VoteMessage::from_solana_message(&m) {
                        Ok(m) => {
                            m.log();
                            self.vote_messages.push(m);
                        }
                        Err(e) => error!("err {:?}", e),
                    },
                    MessageType::Ping => match PingMessage::from_solana_message(&m) {
                        Ok(pm) => {
                            pm.log();
                            self.send_ack(m.receiver, m.sender);
                        }
                        Err(e) => error!("err {:?}", e),
                    },
                    MessageType::ACK => match AckMessage::from_solana_message(&m) {
                        Ok(am) => {
                            am.log(&m);
                        }
                        Err(e) => error!("err {:?}", e),
                    },
                    _ => warn!("message type not implemented"),
                }
            }

            Ok(())
        }

        // fn get_msgs_by_type(&self, msg_type: MessageType) -> &[VoteMessage] {
        //     match msg_type {
        //         MessageType::Vote => self.vote_messages.as_slice(),
        //         _ => panic!("not impl"),
        //     }
        // }

        // fn get_vote_messages_by_block_hash(&self, block_hash: [u8; 64]) -> Vec<&VoteMessage> {
        //     self.vote_messages
        //         .iter()
        //         .filter(|v| v.block == block_hash)
        //         .collect::<Vec<_>>()
        // }

        // fn get_vote_messages_by_slot(&self, slot: Slot) -> Vec<&VoteMessage> {
        //     self.vote_messages
        //         .iter()
        //         .filter(|v| v.slot == slot)
        //         .collect::<Vec<_>>()
        // }
    }
}
