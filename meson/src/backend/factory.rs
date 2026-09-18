//! Select [`FileByteBackend`](s) from process environment.

use super::{FileByteBackend, FileStoreError};
use std::sync::Arc;
use thiserror::Error;

/// Errors from [`blob_store_from_env`] / [`blob_stores_from_env`].
#[derive(Debug, Error)]
pub enum BlobStoreConfigError {
    /// `MESON_BLOB_BACKEND` value is unknown.
    #[error("unknown MESON_BLOB_BACKEND={0}")]
    UnknownBackend(String),
    /// Required env var missing for the selected backend.
    #[error("missing env {0}")]
    MissingEnv(&'static str),
    /// Requested backend was not compiled into this crate.
    #[error("backend {0} not enabled in this build (missing Cargo feature)")]
    FeatureDisabled(&'static str),
    /// Env var present but not parseable.
    #[error("invalid env {0}: {1}")]
    InvalidEnv(&'static str, String),
    /// Underlying store construction failed.
    #[error(transparent)]
    Store(#[from] FileStoreError),
}

/// Dual blob layout: available (post-scan) + quarantine (pre-scan).
#[derive(Clone)]
pub struct BlobStoreLayout {
    /// Bytes readable after a clean scan (or when virus scan is off).
    pub available: Arc<dyn FileByteBackend>,
    /// Bytes held until virus scan completes.
    pub quarantine: Arc<dyn FileByteBackend>,
}

/// Build the **available** store only from `MESON_*` env vars (compat).
///
/// Prefer [`blob_stores_from_env`] when virus scan / quarantine is enabled.
///
/// | `MESON_BLOB_BACKEND` | Backend | Required env |
/// |----------------------|---------|--------------|
/// | `local` (default) | [`LocalDiskBlobStore`](super::LocalDiskBlobStore) | optional `MESON_LOCAL_ROOT` |
/// | `rustfs` | [`RustFsBlobStore`](super::RustFsBlobStore) | `MESON_RUSTFS_ENDPOINT`, `MESON_RUSTFS_BUCKET`, `MESON_RUSTFS_ACCESS_KEY`, `MESON_RUSTFS_SECRET_KEY`; optional `MESON_RUSTFS_REGION`, `MESON_RUSTFS_MAX_BYTES` |
///
/// # Errors
///
/// Missing feature, missing env, or unknown backend name.
pub fn blob_store_from_env() -> Result<Arc<dyn FileByteBackend>, BlobStoreConfigError> {
    Ok(blob_stores_from_env()?.available)
}

/// Build available + quarantine stores from `MESON_*` env vars.
///
/// Local defaults: `MESON_LOCAL_ROOT` → `uploads`, `MESON_LOCAL_QUARANTINE_ROOT` →
/// `uploads-quarantine`. RustFS requires `MESON_RUSTFS_BUCKET` and
/// `MESON_RUSTFS_QUARANTINE_BUCKET`.
///
/// # Errors
///
/// Missing feature, missing env, or unknown backend name.
pub fn blob_stores_from_env() -> Result<BlobStoreLayout, BlobStoreConfigError> {
    let kind = std::env::var("MESON_BLOB_BACKEND").unwrap_or_else(|_| "local".to_string());
    match kind.to_ascii_lowercase().as_str() {
        "local" => {
            #[cfg(feature = "backend-local")]
            {
                use super::LocalDiskBlobStore;
                let root = std::env::var("MESON_LOCAL_ROOT").unwrap_or_else(|_| "uploads".into());
                let qroot = std::env::var("MESON_LOCAL_QUARANTINE_ROOT")
                    .unwrap_or_else(|_| "uploads-quarantine".into());
                Ok(BlobStoreLayout {
                    available: Arc::new(LocalDiskBlobStore::new(root)) as Arc<dyn FileByteBackend>,
                    quarantine: Arc::new(LocalDiskBlobStore::new(qroot))
                        as Arc<dyn FileByteBackend>,
                })
            }
            #[cfg(not(feature = "backend-local"))]
            {
                Err(BlobStoreConfigError::FeatureDisabled("backend-local"))
            }
        }
        "rustfs" => {
            #[cfg(feature = "backend-rustfs")]
            {
                use super::rustfs::{RustFsBlobStore, DEFAULT_RUSTFS_REGION};
                let endpoint = require("MESON_RUSTFS_ENDPOINT")?;
                let bucket = require("MESON_RUSTFS_BUCKET")?;
                let quarantine_bucket = require("MESON_RUSTFS_QUARANTINE_BUCKET")?;
                let access_key = require("MESON_RUSTFS_ACCESS_KEY")?;
                let secret_key = require("MESON_RUSTFS_SECRET_KEY")?;
                let region = std::env::var("MESON_RUSTFS_REGION")
                    .unwrap_or_else(|_| DEFAULT_RUSTFS_REGION.into());
                let max_object_bytes = std::env::var("MESON_RUSTFS_MAX_BYTES")
                    .ok()
                    .filter(|s| !s.is_empty())
                    .map(|s| {
                        s.parse::<usize>().map_err(|_| {
                            BlobStoreConfigError::InvalidEnv(
                                "MESON_RUSTFS_MAX_BYTES",
                                "expected unsigned integer".into(),
                            )
                        })
                    })
                    .transpose()?;
                let available = RustFsBlobStore::new(
                    endpoint.clone(),
                    bucket,
                    access_key.clone(),
                    secret_key.clone(),
                    region.clone(),
                )?
                .with_max_object_bytes(max_object_bytes);
                let quarantine = RustFsBlobStore::new(
                    endpoint,
                    quarantine_bucket,
                    access_key,
                    secret_key,
                    region,
                )?
                .with_max_object_bytes(max_object_bytes);
                Ok(BlobStoreLayout {
                    available: Arc::new(available) as Arc<dyn FileByteBackend>,
                    quarantine: Arc::new(quarantine) as Arc<dyn FileByteBackend>,
                })
            }
            #[cfg(not(feature = "backend-rustfs"))]
            {
                Err(BlobStoreConfigError::FeatureDisabled("backend-rustfs"))
            }
        }
        other => Err(BlobStoreConfigError::UnknownBackend(other.to_string())),
    }
}

#[cfg(feature = "backend-rustfs")]
fn require(name: &'static str) -> Result<String, BlobStoreConfigError> {
    std::env::var(name).map_err(|_| BlobStoreConfigError::MissingEnv(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn env_lock() -> tokio::sync::MutexGuard<'static, ()> {
        static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        LOCK.lock().await
    }

    #[tokio::test]
    async fn local_default_from_env() {
        let _g = env_lock().await;
        std::env::set_var("MESON_BLOB_BACKEND", "local");
        std::env::set_var("MESON_LOCAL_ROOT", "/tmp/meson-blob-test-root");
        std::env::set_var(
            "MESON_LOCAL_QUARANTINE_ROOT",
            "/tmp/meson-blob-test-quarantine",
        );
        let layout = blob_stores_from_env().expect("local");
        let _ = layout.available;
        let _ = layout.quarantine;
        let available_only = blob_store_from_env().expect("compat");
        let _ = available_only;
        std::env::remove_var("MESON_LOCAL_ROOT");
        std::env::remove_var("MESON_LOCAL_QUARANTINE_ROOT");
        std::env::remove_var("MESON_BLOB_BACKEND");
    }

    #[tokio::test]
    async fn unknown_backend_sad() {
        let _g = env_lock().await;
        std::env::set_var("MESON_BLOB_BACKEND", "nope");
        match blob_stores_from_env() {
            Err(BlobStoreConfigError::UnknownBackend(_)) => {}
            Ok(_) => panic!("expected UnknownBackend, got Ok"),
            Err(e) => panic!("expected UnknownBackend, got Err({e})"),
        }
        std::env::remove_var("MESON_BLOB_BACKEND");
    }

    #[cfg(feature = "backend-rustfs")]
    #[tokio::test]
    async fn rustfs_missing_env_sad() {
        let _g = env_lock().await;
        std::env::set_var("MESON_BLOB_BACKEND", "rustfs");
        for key in [
            "MESON_RUSTFS_ENDPOINT",
            "MESON_RUSTFS_BUCKET",
            "MESON_RUSTFS_QUARANTINE_BUCKET",
            "MESON_RUSTFS_ACCESS_KEY",
            "MESON_RUSTFS_SECRET_KEY",
        ] {
            std::env::remove_var(key);
        }
        match blob_stores_from_env() {
            Err(BlobStoreConfigError::MissingEnv(_)) => {}
            Ok(_) => panic!("expected MissingEnv, got Ok"),
            Err(e) => panic!("expected MissingEnv, got Err({e})"),
        }
        std::env::remove_var("MESON_BLOB_BACKEND");
    }

    #[cfg(not(feature = "backend-rustfs"))]
    #[tokio::test]
    async fn rustfs_feature_disabled_sad() {
        let _g = env_lock().await;
        std::env::set_var("MESON_BLOB_BACKEND", "rustfs");
        match blob_stores_from_env() {
            Err(BlobStoreConfigError::FeatureDisabled("backend-rustfs")) => {}
            Ok(_) => panic!("expected FeatureDisabled, got Ok"),
            Err(e) => panic!("expected FeatureDisabled, got Err({e})"),
        }
        std::env::remove_var("MESON_BLOB_BACKEND");
    }
}
