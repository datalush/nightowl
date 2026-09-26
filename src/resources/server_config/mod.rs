//! Renderers for the Fluss `server.yaml` content (the letter, not the envelope).
//!
//! Each module contributes one concern through `properties()`; `config_map.rs`
//! assembles them per role (coordinator vs tablet server).

pub mod listeners;
pub mod storage;
pub mod table_defaults;
pub mod zookeeper;
