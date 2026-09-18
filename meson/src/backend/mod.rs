//! Byte backends for Valence [`File`](crate) trait payloads.
//!
//! Valence stores metadata and an opaque `storage_path` key; this module puts
//! and gets the bytes. Embedded hosts use [`LocalDiskBlobStore`]; remote hosts
//! use [`RustFsBlobStore`] (`backend-rustfs`) against a RustFS endpoint from
//! `MESON_*` env at boot.

pub(crate) mod keys;

#[cfg(feature = "backend-rustfs")]
mod sigv4;

#[cfg(feature = "backend-rustfs")]
pub mod rustfs;

pub mod factory;

use async_trait::async_trait;
use std::path::PathBuf;
use thiserror::Error;
use tokio::fs;
use tokio::io::AsyncWriteExt;

pub use factory::{
    blob_store_from_env, blob_stores_from_env, BlobStoreConfigError, BlobStoreLayout,
};

#[cfg(feature = "backend-rustfs")]
pub use rustfs::{RustFsBlobStore, DEFAULT_RUSTFS_REGION};

/// Errors from [`FileByteBackend`] operations.
#[derive(Debug, Error)]
pub enum FileStoreError {
    /// Object key was missing or not readable.
    #[error("not found")]
    NotFound,
    /// Filesystem or I/O failure (Display stays opaque; inspect [`source`](std::error::Error::source)).
    #[error("storage I/O error")]
    Io {
        /// HTTP status from a remote store, when known (timeouts / 429 / 5xx aid retry policy).
        http_status: Option<u16>,
        /// Underlying cause (filesystem, HTTP client, etc.); never logged as Display of this variant.
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },
    /// Key escapes the store root or is otherwise invalid.
    #[error("invalid storage key")]
    InvalidKey,
    /// Credentials rejected by the remote store.
    #[error("storage unauthorized")]
    Unauthorized,
    /// Host has not called [`crate::install_blob_store`] yet.
    #[error("blob store not installed")]
    BlobStoreNotInstalled,
    /// Host has not called [`crate::install_quarantine_store`] yet.
    #[error("quarantine store not installed")]
    QuarantineStoreNotInstalled,
    /// File bytes are not readable because status is not `Available`.
    ///
    /// Optional `status` is the opaque enum wire value (no filenames).
    #[error("not available")]
    NotAvailable {
        /// Wire form of `file_status` when known (for example `pending_virus_scan`).
        status: Option<String>,
    },
}

impl FileStoreError {
    /// Wrap a typed source as opaque I/O failure.
    #[must_use]
    pub fn io(source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Io {
            http_status: None,
            source: Some(Box::new(source)),
        }
    }

    /// I/O failure with only a message (no typed source).
    #[must_use]
    pub fn io_msg(msg: impl Into<String>) -> Self {
        Self::Io {
            http_status: None,
            source: Some(Box::new(std::io::Error::other(msg.into()))),
        }
    }

    /// Remote HTTP failure classified by status (Display remains opaque).
    #[must_use]
    pub fn io_http(status: u16) -> Self {
        Self::Io {
            http_status: Some(status),
            source: None,
        }
    }

    /// Remote HTTP failure with an underlying client/body error.
    #[must_use]
    pub fn io_http_source(
        status: u16,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::Io {
            http_status: Some(status),
            source: Some(Box::new(source)),
        }
    }

    /// Whether a host may reasonably retry (timeouts, 429, 5xx).
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Io {
                http_status: Some(status),
                ..
            } => matches!(*status, 408 | 429 | 500..=599),
            Self::Io {
                http_status: None,
                source: Some(source),
            } => {
                #[cfg(feature = "backend-rustfs")]
                if source
                    .downcast_ref::<reqwest::Error>()
                    .is_some_and(|e| e.is_timeout() || e.is_connect())
                {
                    return true;
                }
                source.downcast_ref::<std::io::Error>().is_some_and(|e| {
                    matches!(
                        e.kind(),
                        std::io::ErrorKind::TimedOut
                            | std::io::ErrorKind::ConnectionRefused
                            | std::io::ErrorKind::ConnectionReset
                            | std::io::ErrorKind::ConnectionAborted
                            | std::io::ErrorKind::Interrupted
                    )
                })
            }
            _ => false,
        }
    }
}

/// Put / get / delete opaque object keys for File-backed records.
#[async_trait]
pub trait FileByteBackend: Send + Sync {
    /// Store `bytes` under `key` (overwrite if present).
    ///
    /// # Errors
    ///
    /// Returns [`FileStoreError::InvalidKey`] when `key` is empty, absolute, or
    /// contains path separators / `..`. Returns [`FileStoreError::Io`] when the
    /// underlying store cannot write. Remote stores may return
    /// [`FileStoreError::Unauthorized`].
    async fn put(&self, key: &str, bytes: &[u8]) -> Result<(), FileStoreError>;

    /// Load bytes for `key`.
    ///
    /// # Errors
    ///
    /// Returns [`FileStoreError::InvalidKey`] for unsafe keys,
    /// [`FileStoreError::NotFound`] when the object is missing, and
    /// [`FileStoreError::Io`] for other read failures.
    async fn get(&self, key: &str) -> Result<Vec<u8>, FileStoreError>;

    /// Delete `key` if present (missing key is ok).
    ///
    /// # Errors
    ///
    /// Returns [`FileStoreError::InvalidKey`] for unsafe keys and
    /// [`FileStoreError::Io`] when delete fails for a reason other than missing.
    async fn delete(&self, key: &str) -> Result<(), FileStoreError>;
}

/// Local directory blob store (`uploads/` by default).
#[cfg(feature = "backend-local")]
#[derive(Debug, Clone)]
pub struct LocalDiskBlobStore {
    root: PathBuf,
}

#[cfg(feature = "backend-local")]
impl LocalDiskBlobStore {
    /// Create a store rooted at `root` (created on first put).
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Default process-local `uploads` directory.
    #[must_use]
    pub fn default_uploads() -> Self {
        Self::new("uploads")
    }

    fn resolve(&self, key: &str) -> Result<PathBuf, FileStoreError> {
        keys::validate_object_key(key)?;
        Ok(self.root.join(key))
    }
}

#[cfg(feature = "backend-local")]
#[async_trait]
impl FileByteBackend for LocalDiskBlobStore {
    async fn put(&self, key: &str, bytes: &[u8]) -> Result<(), FileStoreError> {
        let path = self.resolve(key)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await.map_err(|e| {
                tracing::warn!(backend = "local", outcome = "create_dir", error = %e);
                FileStoreError::io(e)
            })?;
        }
        let mut file = fs::File::create(&path).await.map_err(|e| {
            tracing::warn!(backend = "local", outcome = "create_file", error = %e);
            FileStoreError::io(e)
        })?;
        file.write_all(bytes).await.map_err(|e| {
            tracing::warn!(backend = "local", outcome = "write", error = %e);
            FileStoreError::io(e)
        })?;
        file.flush().await.map_err(|e| {
            tracing::warn!(backend = "local", outcome = "flush", error = %e);
            FileStoreError::io(e)
        })?;
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Vec<u8>, FileStoreError> {
        let path = self.resolve(key)?;
        fs::read(&path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                FileStoreError::NotFound
            } else {
                tracing::warn!(backend = "local", outcome = "read", error = %e);
                FileStoreError::io(e)
            }
        })
    }

    async fn delete(&self, key: &str) -> Result<(), FileStoreError> {
        let path = self.resolve(key)?;
        match fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => {
                tracing::warn!(backend = "local", outcome = "delete", error = %e);
                Err(FileStoreError::io(e))
            }
        }
    }
}

#[cfg(all(test, feature = "backend-local"))]
mod tests {
    use super::*;

    fn temp_store() -> (PathBuf, LocalDiskBlobStore) {
        let root = std::env::temp_dir().join(format!(
            "meson-file-store-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        (root.clone(), LocalDiskBlobStore::new(root))
    }

    #[tokio::test]
    async fn local_disk_put_get_delete_happy() {
        let (root, store) = temp_store();
        store.put("a.png", b"hello").await.unwrap();
        assert_eq!(store.get("a.png").await.unwrap(), b"hello");
        store.delete("a.png").await.unwrap();
        assert!(matches!(
            store.get("a.png").await.unwrap_err(),
            FileStoreError::NotFound
        ));
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn local_disk_rejects_path_escape_sad() {
        let (root, store) = temp_store();
        assert!(matches!(
            store.put("../x.png", b"x").await.unwrap_err(),
            FileStoreError::InvalidKey
        ));
        assert!(matches!(
            store.put("a/b.png", b"x").await.unwrap_err(),
            FileStoreError::InvalidKey
        ));
        assert!(matches!(
            store.put("a\\b.png", b"x").await.unwrap_err(),
            FileStoreError::InvalidKey
        ));
        assert!(matches!(
            store.put("", b"x").await.unwrap_err(),
            FileStoreError::InvalidKey
        ));
        assert!(matches!(
            store.put("/abs.png", b"x").await.unwrap_err(),
            FileStoreError::InvalidKey
        ));
        let _ = std::fs::remove_dir_all(root);
    }
}

#[cfg(test)]
mod error_tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn io_preserves_source_chain() {
        let err = FileStoreError::io(std::io::Error::other("disk full"));
        assert_eq!(err.to_string(), "storage I/O error");
        let source = err.source().expect("source");
        assert!(source.to_string().contains("disk full"));
    }

    #[test]
    fn io_http_retryable_classification() {
        assert!(FileStoreError::io_http(503).is_retryable());
        assert!(FileStoreError::io_http(429).is_retryable());
        assert!(!FileStoreError::io_http(400).is_retryable());
        assert!(!FileStoreError::NotFound.is_retryable());
    }
}
