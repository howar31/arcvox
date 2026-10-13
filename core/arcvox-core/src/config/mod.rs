//! Configuration: data model, validation, and the on-disk store.

pub mod base_prompt;
pub mod engines;
pub mod model;
pub mod store;
pub mod validate;

pub use engines::{ensure_referenced_engines, sync_engines};
pub use model::*;
pub use store::{ConfigStore, StoreError};
pub use validate::{validate, ConfigError};
