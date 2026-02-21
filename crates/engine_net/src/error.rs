//! Network-layer error types.

/// Errors that can occur during network operations.
#[derive(Debug, thiserror::Error)]
pub enum NetError {
    /// Failed to encode a message to MessagePack.
    #[error("failed to encode message: {0}")]
    Encode(#[from] rmp_serde::encode::Error),

    /// Failed to decode a message from MessagePack.
    #[error("failed to decode message: {0}")]
    Decode(#[from] rmp_serde::decode::Error),

    /// NATS connection or publish error.
    #[error("NATS error: {0}")]
    Nats(String),

    /// NATS subscription error.
    #[error("NATS subscribe error: {0}")]
    Subscribe(#[from] async_nats::SubscribeError),

    /// NATS publish error.
    #[error("NATS publish error: {0}")]
    Publish(#[from] async_nats::PublishError),

    /// NATS connection error.
    #[error("NATS connection error: {0}")]
    Connect(#[from] async_nats::ConnectError),

    /// A required NATS header was missing.
    #[error("missing NATS header: {0}")]
    MissingHeader(String),
}
