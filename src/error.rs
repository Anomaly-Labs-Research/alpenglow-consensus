use std::fmt::{Debug, Display, write};

pub enum AlpenGlowError {
    InvalidKeypair,
    InvalidQuicConfig,
    InvalidMessage,
}

pub type AlpenGlowResult<T> = Result<T, AlpenGlowError>;

impl Debug for AlpenGlowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        let err_str = match self {
            AlpenGlowError::InvalidKeypair => "Invalid Keypair",
            AlpenGlowError::InvalidQuicConfig => "Invalid Quic Config",
            AlpenGlowError::InvalidMessage => "Invalid Message",
        };

        write(f, format_args!("{}", err_str))
    }
}

impl Display for AlpenGlowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        let err_str = match self {
            AlpenGlowError::InvalidKeypair => "Invalid Keypair",
            AlpenGlowError::InvalidQuicConfig => "Invalid Quic Config",
            AlpenGlowError::InvalidMessage => "Invalid Message",
        };

        write(f, format_args!("{}", err_str))
    }
}
