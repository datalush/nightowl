//! One module per remote-storage backend, each rendering its own
//! `server.yaml` properties from its own spec type.
//!
//! The dispatcher in `super` picks the module; backends never call each
//! other. Shared helpers genuinely backend-independent (e.g. the generic
//! secret markers, when implemented) will live in a `common` module here.

pub mod s3;
