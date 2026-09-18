//! Promote quarantine object bytes into the available store.

use crate::blob_install::{installed_blob_store, installed_quarantine_store};
use crate::FileStoreError;
use thiserror::Error;

/// Errors from [`promote_to_available`].
#[derive(Debug, Error)]
pub enum PromoteError {
    /// Available put succeeded but quarantine delete failed (orphan possible).
    #[error("promote partial: quarantine delete failed")]
    Partial {
        /// Underlying store error.
        #[source]
        source: FileStoreError,
    },
    /// Store install or get/put failure before a partial state.
    #[error(transparent)]
    Store(#[from] FileStoreError),
}

/// Copy `key` from quarantine → available, then delete quarantine.
///
/// Returns the same opaque key as `storage_path` (preferred when both stores
/// share the key namespace).
///
/// # Errors
///
/// [`PromoteError::Store`] when get/put fails or a store is not installed.
/// [`PromoteError::Partial`] when available put succeeded but quarantine delete
/// failed (bytes may exist in both stores).
pub async fn promote_to_available(key: &str) -> Result<String, PromoteError> {
    tracing::debug!(
        target: "meson.promote",
        operation = "promote_to_available",
        "promoting quarantine object"
    );
    let quarantine = installed_quarantine_store()?;
    let available = installed_blob_store()?;
    let bytes = quarantine.get(key).await?;
    available.put(key, &bytes).await?;
    match quarantine.delete(key).await {
        Ok(()) => {
            tracing::info!(
                target: "meson.promote",
                operation = "promote_to_available",
                outcome = "ok",
                "promoted object to available store"
            );
            Ok(key.to_string())
        }
        Err(source) => {
            tracing::warn!(
                target: "meson.promote",
                operation = "promote_to_available",
                outcome = "partial_delete_failed",
                "available put ok; quarantine delete failed"
            );
            Err(PromoteError::Partial { source })
        }
    }
}

#[cfg(all(test, feature = "backend-local"))]
mod tests {
    use super::*;
    use crate::blob_install::{
        blob_stores_unit_test_lock, clear_blob_stores_for_test, install_blob_store,
        install_quarantine_store,
    };
    use crate::LocalDiskBlobStore;
    use std::sync::Arc;

    fn temp_pair() -> (std::path::PathBuf, std::path::PathBuf) {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let avail = std::env::temp_dir().join(format!("meson-promote-a-{n}"));
        let quar = std::env::temp_dir().join(format!("meson-promote-q-{n}"));
        (avail, quar)
    }

    #[tokio::test]
    async fn promote_happy() {
        let _g = blob_stores_unit_test_lock().await;
        clear_blob_stores_for_test();
        let (a, q) = temp_pair();
        install_blob_store(Arc::new(LocalDiskBlobStore::new(a.clone()))).unwrap();
        install_quarantine_store(Arc::new(LocalDiskBlobStore::new(q.clone()))).unwrap();
        let quar = installed_quarantine_store().unwrap();
        quar.put("x.png", b"clean").await.unwrap();
        let path = promote_to_available("x.png").await.unwrap();
        assert_eq!(path, "x.png");
        let avail = installed_blob_store().unwrap();
        assert_eq!(avail.get("x.png").await.unwrap(), b"clean");
        assert!(matches!(
            quar.get("x.png").await.unwrap_err(),
            FileStoreError::NotFound
        ));
        clear_blob_stores_for_test();
        let _ = std::fs::remove_dir_all(a);
        let _ = std::fs::remove_dir_all(q);
    }

    #[tokio::test]
    async fn promote_quarantine_not_installed_sad() {
        let _g = blob_stores_unit_test_lock().await;
        clear_blob_stores_for_test();
        let a = std::env::temp_dir().join(format!(
            "meson-promote-only-a-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        install_blob_store(Arc::new(LocalDiskBlobStore::new(a.clone()))).unwrap();
        let err = promote_to_available("x.png").await.unwrap_err();
        assert!(
            matches!(
                err,
                PromoteError::Store(FileStoreError::QuarantineStoreNotInstalled)
            ),
            "got {err:?}"
        );
        clear_blob_stores_for_test();
        let _ = std::fs::remove_dir_all(a);
    }
}
