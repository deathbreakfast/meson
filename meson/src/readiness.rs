//! Wait / gate helpers until a File is Available for processing.

use crate::blob_install::installed_blob_store;
use crate::file_scan_adapter::{file_scan_adapter, FileScanAdapterError, FileScanSnapshot};
use crate::generated::FileFileStatus;
use crate::FileStoreError;
use std::time::Duration;
use thiserror::Error;
use valence::Valence;

/// Errors from readiness polling / Available gates.
#[derive(Debug, Error)]
pub enum ReadinessError {
    /// Timed out while waiting for Available.
    #[error("timed out waiting for available")]
    Timeout,
    /// File reached a terminal non-readable status (for example quarantined).
    #[error("not available")]
    NotAvailable {
        /// Opaque status wire value.
        status: Option<String>,
    },
    /// Adapter / Valence failure.
    #[error(transparent)]
    Adapter(#[from] FileScanAdapterError),
    /// Blob store failure.
    #[error(transparent)]
    Store(#[from] FileStoreError),
}

fn poll_interval() -> Duration {
    Duration::from_millis(50)
}

/// Poll Valence until `file_id` on `table` is `Available`, or fail.
///
/// # Errors
///
/// [`ReadinessError::Timeout`], [`ReadinessError::NotAvailable`] on Quarantined,
/// or adapter/store errors.
pub async fn wait_until_available(
    valence: &Valence,
    table: &str,
    file_id: &str,
    timeout: Duration,
) -> Result<FileScanSnapshot, ReadinessError> {
    let adapter = file_scan_adapter(table)?;
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let snap = adapter.load(valence, file_id).await?;
        match snap.file_status {
            FileFileStatus::Available => return Ok(snap),
            FileFileStatus::Quarantined => {
                return Err(ReadinessError::NotAvailable {
                    status: Some(snap.file_status.as_str().to_string()),
                });
            }
            FileFileStatus::PendingVirusScan | FileFileStatus::VirusScanComplete => {
                if tokio::time::Instant::now() >= deadline {
                    return Err(ReadinessError::Timeout);
                }
                tokio::time::sleep(poll_interval()).await;
            }
        }
    }
}

/// Poll the **available** store until `storage_path` exists, or timeout.
///
/// # Errors
///
/// [`ReadinessError::Timeout`] or store errors other than NotFound while polling.
pub async fn wait_until_available_object(
    storage_path: &str,
    timeout: Duration,
) -> Result<Vec<u8>, ReadinessError> {
    let store = installed_blob_store()?;
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match store.get(storage_path).await {
            Ok(bytes) => return Ok(bytes),
            Err(FileStoreError::NotFound) => {
                if tokio::time::Instant::now() >= deadline {
                    return Err(ReadinessError::Timeout);
                }
                tokio::time::sleep(poll_interval()).await;
            }
            Err(e) => return Err(ReadinessError::Store(e)),
        }
    }
}

/// Load Available file bytes via adapter + available store.
///
/// # Errors
///
/// [`ReadinessError::NotAvailable`] when status is not Available, or store/adapter errors.
pub async fn get_available_file_bytes(
    valence: &Valence,
    table: &str,
    file_id: &str,
) -> Result<Vec<u8>, ReadinessError> {
    let adapter = file_scan_adapter(table)?;
    let snap = adapter.load(valence, file_id).await?;
    ensure_status_available(&snap)?;
    let store = installed_blob_store()?;
    Ok(store.get(&snap.storage_path).await?)
}

/// Ensure a snapshot is Available (processors / OCR gates).
///
/// # Errors
///
/// [`ReadinessError::NotAvailable`] when status is not Available.
pub fn ensure_readable_for_processing(snap: &FileScanSnapshot) -> Result<(), ReadinessError> {
    ensure_status_available(snap)
}

fn ensure_status_available(snap: &FileScanSnapshot) -> Result<(), ReadinessError> {
    if matches!(snap.file_status, FileFileStatus::Available) {
        Ok(())
    } else {
        Err(ReadinessError::NotAvailable {
            status: Some(snap.file_status.as_str().to_string()),
        })
    }
}

#[cfg(all(test, feature = "backend-local"))]
mod tests {
    use super::*;
    use crate::blob_install::{
        blob_stores_unit_test_lock, clear_blob_stores_for_test, install_blob_store,
    };
    use crate::LocalDiskBlobStore;
    use std::sync::Arc;

    #[tokio::test]
    async fn wait_object_timeout_sad() {
        let _g = blob_stores_unit_test_lock().await;
        clear_blob_stores_for_test();
        let root = std::env::temp_dir().join(format!(
            "meson-ready-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        install_blob_store(Arc::new(LocalDiskBlobStore::new(root.clone()))).unwrap();
        let err = wait_until_available_object("missing.png", Duration::from_millis(80))
            .await
            .unwrap_err();
        assert!(matches!(err, ReadinessError::Timeout), "got {err:?}");
        clear_blob_stores_for_test();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn ensure_quarantined_sad() {
        let snap = FileScanSnapshot {
            storage_path: "x.png".into(),
            file_status: FileFileStatus::Quarantined,
            uploaded_by: valence::RecordId::new("user", "u"),
        };
        let err = ensure_readable_for_processing(&snap).unwrap_err();
        assert!(matches!(err, ReadinessError::NotAvailable { .. }));
    }
}
