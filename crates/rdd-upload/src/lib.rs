//! Storage de objetos neutral para `rdd` (§10.13, ADR-34).
//!
//! El puerto [`ObjectStorage`] y [`MemoryStorage`] no conocen ninguna nube. Las
//! features `aws`, `azure` y `gcp` añaden un adaptador REST que firma sus
//! peticiones a mano sobre un cliente `hyper` + `hyper-rustls` compartido, sin
//! SDK de nube (RA-025).
//!
//! ```no_run
//! use rdd_upload::{MemoryStorage, ObjectKey, ObjectStorage, PutOptions};
//!
//! # async fn demo() -> rdd_core::Result<()> {
//! let storage = MemoryStorage::new();
//! let key = ObjectKey::parse("files/uno.txt").unwrap();
//! storage
//!     .put(&key, bytes::Bytes::from_static(b"hola"), PutOptions::new())
//!     .await?;
//! # Ok(())
//! # }
//! ```

mod error;
mod key;
mod memory;
mod meta;
mod port;
mod secret;

#[cfg(any(feature = "aws", feature = "azure", feature = "gcp"))]
mod endpoint;
#[cfg(any(feature = "aws", feature = "azure", feature = "gcp"))]
mod http_client;
#[cfg(any(feature = "aws", feature = "azure", feature = "gcp"))]
mod sign;

#[cfg(feature = "azure")]
mod azure;
#[cfg(feature = "gcp")]
mod gcs;
#[cfg(feature = "aws")]
mod s3;

pub use error::UploadConfigError;
pub use key::{ObjectKey, ObjectKeyError};
pub use memory::{DEFAULT_MAX_OBJECT_BYTES, MemoryStorage};
pub use meta::{ObjectMeta, PutOptions, StoredObject};
pub use port::{BoxFuture, ObjectStorage};
pub use secret::{Credentials, Secret};

#[cfg(feature = "azure")]
pub use azure::{AzureBlobConfig, AzureBlobStorage};
#[cfg(feature = "gcp")]
pub use gcs::{GcsConfig, GcsStorage};
#[cfg(feature = "aws")]
pub use s3::{S3Config, S3Storage};

pub use rdd_core;
