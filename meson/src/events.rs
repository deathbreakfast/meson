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
