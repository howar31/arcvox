//! Model catalogue: the remote manifest, the on-disk store, downloads and
//! custom imports. Model files never live under the install directory.

pub mod download;
pub mod magic;
pub mod manifest;
pub mod store;

pub use manifest::{Manifest, ManifestSource, ModelEntry, ModelRole};
pub use store::{InstalledModel, ModelStore};

#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("model `{0}` is not in the catalogue")]
    NotInCatalogue(String),
    #[error("model `{0}` is not installed")]
    NotInstalled(String),
    #[error("file is neither a ggml nor a GGUF model")]
    UnknownFormat,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("manifest: {0}")]
    Manifest(String),
    #[error("download: {0}")]
    Http(String),
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    Sha256Mismatch { expected: String, actual: String },
    #[error("not enough free disk space: need {needed} bytes, {free} free")]
    InsufficientDisk { needed: u64, free: u64 },
    #[error("download cancelled")]
    Cancelled,
}
