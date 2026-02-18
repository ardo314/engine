//! NATS connection management.
//!
//! Provides a thin wrapper around `async-nats` for connecting to NATS with
//! engine-specific defaults.

use tracing::info;

use crate::error::NetError;

/// Default NATS server URL.
pub const DEFAULT_NATS_URL: &str = "nats://localhost:4222";

/// The environment variable used to override the NATS URL.
pub const NATS_URL_ENV: &str = "NATS_URL";

/// A wrapper around an `async-nats` client with engine-specific helpers.
#[derive(Debug, Clone)]
pub struct NatsConnection {
    /// The underlying NATS client.
    client: async_nats::Client,
}

impl NatsConnection {
    /// Connect to NATS using the URL from the `NATS_URL` environment variable,
    /// falling back to [`DEFAULT_NATS_URL`].
    ///
    /// # Errors
    ///
    /// Returns [`NetError::Connect`] if the connection cannot be established.
    pub async fn connect() -> Result<Self, NetError> {
        let url = std::env::var(NATS_URL_ENV).unwrap_or_else(|_| DEFAULT_NATS_URL.to_string());
        Self::connect_to(&url).await
    }

    /// Connect to NATS at the specified URL.
    ///
    /// # Errors
    ///
    /// Returns [`NetError::Connect`] if the connection cannot be established.
    pub async fn connect_to(url: &str) -> Result<Self, NetError> {
        info!(url, "connecting to NATS");
        let client = async_nats::connect(url).await?;
        info!("NATS connection established");
        Ok(Self { client })
    }

    /// Returns a reference to the underlying `async-nats` client.
    #[must_use]
    pub fn client(&self) -> &async_nats::Client {
        &self.client
    }

    /// Publish a MessagePack-encoded message to a subject.
    ///
    /// # Errors
    ///
    /// Returns [`NetError`] if encoding or publishing fails.
    pub async fn publish<T: serde::Serialize>(
        &self,
        subject: &str,
        message: &T,
    ) -> Result<(), NetError> {
        let payload = crate::codec::encode(message)?;
        self.client
            .publish(subject.to_string(), payload.into())
            .await?;
        Ok(())
    }

    /// Publish a MessagePack-encoded message with NATS headers.
    ///
    /// # Errors
    ///
    /// Returns [`NetError`] if encoding or publishing fails.
    pub async fn publish_with_headers<T: serde::Serialize>(
        &self,
        subject: &str,
        headers: async_nats::HeaderMap,
        message: &T,
    ) -> Result<(), NetError> {
        let payload = crate::codec::encode(message)?;
        self.client
            .publish_with_headers(subject.to_string(), headers, payload.into())
            .await?;
        Ok(())
    }

    /// Subscribe to a subject.
    ///
    /// # Errors
    ///
    /// Returns [`NetError::Subscribe`] if the subscription fails.
    pub async fn subscribe(&self, subject: &str) -> Result<async_nats::Subscriber, NetError> {
        let sub = self.client.subscribe(subject.to_string()).await?;
        Ok(sub)
    }
}
