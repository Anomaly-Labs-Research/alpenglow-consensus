use std::thread::{self, JoinHandle};

use crossbeam::channel::Receiver;
use tracing::info;

use crate::error::{AlpenGlowError, AlpenGlowResult};

pub const MAX_MESSAGE_LEN_BYTES: u16 = 1024; // 1MB

pub trait TxMessage {
    type MessageData;
    fn pack(&self) -> Vec<u8>;
    fn unpack(bytes: &[u8]) -> AlpenGlowResult<Self::MessageData>;
}

#[repr(C)]
#[derive(Debug)]
pub struct AlpenGlowMessage {
    message_len_in_bytes: u16,
    data: String,
}

impl AlpenGlowMessage {
    pub fn from_str(data: &str) -> AlpenGlowMessage {
        Self {
            message_len_in_bytes: data.len() as u16,
            data: data.to_string(),
        }
    }
}

impl TxMessage for AlpenGlowMessage {
    type MessageData = String;
    fn pack(&self) -> Vec<u8> {
        let len_bytes: [u8; 2] = self.message_len_in_bytes.to_le_bytes();
        let mut buffer = Vec::with_capacity(2 + self.message_len_in_bytes as usize);

        buffer.extend_from_slice(&len_bytes);

        buffer.extend_from_slice(self.data.as_bytes());

        buffer
    }

    fn unpack(bytes: &[u8]) -> AlpenGlowResult<Self::MessageData> {
        if bytes.len() as u16 > MAX_MESSAGE_LEN_BYTES {
            return Err(AlpenGlowError::InvalidMessage);
        }
        String::from_utf8(bytes.to_vec()).map_err(|_| AlpenGlowError::InvalidMessage)
    }
}

pub struct MessageProcesser;

impl MessageProcesser {
    pub fn spawn_with_receiver(rx: Receiver<Vec<u8>>) -> JoinHandle<()> {
        info!("spawn : messagsProcesserThread");
        thread::spawn(move || {
            while let Ok(m) = rx.recv() {
                match AlpenGlowMessage::unpack(m.as_slice()) {
                    Ok(d) => println!("data : {}", d),
                    Err(e) => println!("err {:?}", e),
                }
            }
        })
    }
}
