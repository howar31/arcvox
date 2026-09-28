//! Configuration: data model, validation, and the on-disk store.

pub mod model;
pub mod validate;

pub use model::*;
pub use validate::{validate, ConfigError};
