use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid node: {0}")]
    InvalidNode(String),

    #[error("unsupported node version: {0}")]
    UnsupportedVersion(u8),

    #[error("unknown node type: 0x{0:08x}")]
    UnknownType(u32),

    #[error("node not found: {0}")]
    NotFound(String),

    #[error("invalid pack: {0}")]
    InvalidPack(String),

    #[error("pack checksum mismatch")]
    PackChecksum,

    #[error("unsupported codec: {0}")]
    UnsupportedCodec(u8),

    #[error("invalid hex: {0}")]
    Hex(String),

    #[error("config: {0}")]
    Config(String),

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
