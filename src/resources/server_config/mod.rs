//! Renderers for the Fluss `server.yaml` content (the letter, not the envelope).
//!
//! Each module contributes one concern through `properties()`; `config_map.rs`
//! assembles them per role (coordinator vs tablet server).

pub mod listeners;
pub mod overrides;
pub mod storage;
pub mod table_defaults;
pub mod zookeeper;

use overrides::ForbiddenKey;

/// Shared error for the whole `server.yaml` rendering layer: builders
/// return it, the reconciler step maps it to observations.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error(transparent)]
    Forbidden(#[from] ForbiddenKey),
    #[error("invalid value {value:?} for key {key:?}: {reason}")]
    InvalidValue {
        key: String,
        value: String,
        reason: String,
    },
}
