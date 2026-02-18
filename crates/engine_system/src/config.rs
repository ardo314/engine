//! System configuration.

use engine_component::QueryDescriptor;

/// Configuration for a system process.
#[derive(Debug, Clone)]
pub struct SystemConfig {
    /// Human-readable system name (e.g. `"physics"`).
    pub name: String,
    /// The data access requirements of this system.
    pub query: QueryDescriptor,
    /// Optional NATS URL override (defaults to `NATS_URL` env or localhost).
    pub nats_url: Option<String>,
}

impl SystemConfig {
    /// Create a new system config with the given name and query.
    #[must_use]
    pub fn new(name: impl Into<String>, query: QueryDescriptor) -> Self {
        Self {
            name: name.into(),
            query,
            nats_url: None,
        }
    }

    /// Override the NATS URL for this system.
    #[must_use]
    pub fn with_nats_url(mut self, url: impl Into<String>) -> Self {
        self.nats_url = Some(url.into());
        self
    }
}
