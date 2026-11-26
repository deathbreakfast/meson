//! Photon topic for File status changes after virus scan.

/// Published when a File's `file_status` changes (keyed by owner user id).
#[cfg(feature = "photon")]
#[photon::topic(name = "meson.file.updated", keyed_by = "user_id")]
pub struct FileUpdated {
    /// Session user key (`auth = "user"` WS).
    pub user_id: String,
    /// Bare File id.
    pub file_id: String,
    /// New status string (`available`, `quarantined`, …).
    pub file_status: String,
}

/// Best-effort publish of [`FileUpdated`].
pub async fn publish_file_updated(user_id: &str, file_id: &str, file_status: &str) {
    #[cfg(feature = "photon")]
    {
        if user_id.is_empty() || user_id == "system" {
            return;
        }
        match (FileUpdated {
            user_id: user_id.to_string(),
            file_id: file_id.to_string(),
            file_status: file_status.to_string(),
        })
        .publish()
        .await
        {
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(
                    target: "meson.photon.publish",
                    file_id = %file_id,
                    error = %e,
                    "meson.file.updated publish failed"
                );
            }
        }
    }
    #[cfg(not(feature = "photon"))]
    {
        let _ = (user_id, file_id, file_status);
    }
}

/// Published when a background zip is stored and ready to download (keyed by owner user id).
#[cfg(feature = "photon")]
#[photon::topic(name = "meson.zip.ready", keyed_by = "user_id")]
pub struct ZipReady {
    /// Owner's user id.
    pub user_id: String,
    /// Boson task id returned by [`crate::zip_files`].
    pub task_id: String,
    /// Available-store key of the zip ([`crate::zip_object_key`]).
    pub object_key: String,
    /// RFC 3339 time after which the download link should stop working.
    pub expires_at: String,
    /// Files in the zip.
    pub files_included: u64,
    /// Files left out (quarantined, scanning, missing).
    pub files_skipped: u64,
}

/// Best-effort publish of [`ZipReady`]. Does nothing without the `photon` feature.
pub async fn publish_zip_ready(
    user_id: &str,
    task_id: &str,
    object_key: &str,
    expires_at: chrono::DateTime<chrono::Utc>,
    files_included: usize,
    files_skipped: usize,
) {
    #[cfg(feature = "photon")]
    {
        let event = ZipReady {
            user_id: user_id.to_string(),
            task_id: task_id.to_string(),
            object_key: object_key.to_string(),
            expires_at: expires_at.to_rfc3339(),
            files_included: files_included as u64,
            files_skipped: files_skipped as u64,
        };
        if let Err(e) = event.publish().await {
            tracing::warn!(
                target: "meson.photon.publish",
                task_id = %task_id,
                error = %e,
                "meson.zip.ready publish failed"
            );
        }
    }
    #[cfg(not(feature = "photon"))]
    {
        let _ = (
            user_id,
            task_id,
            object_key,
            expires_at,
            files_included,
            files_skipped,
        );
    }
}
