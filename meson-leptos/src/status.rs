//! Status → src / badge helpers (pure; unit-tested).

/// Strip a Valence `table:id` down to the bare id used by `/api/files/{id}`.
#[must_use]
pub fn bare_file_id(file_id: &str) -> &str {
    file_id
        .rsplit_once(':')
        .map(|(_, id)| id)
        .unwrap_or(file_id)
}

/// `src` for Orbital Image: `/api/files/{bare}` only when status is `available`.
#[must_use]
pub fn meson_image_src_for_status(file_id: &str, file_status: &str) -> Option<String> {
    if file_status == "available" {
        Some(format!("/api/files/{}", bare_file_id(file_id)))
    } else {
        None
    }
}

/// True when the File is still awaiting scan / promote (show Skeleton).
#[must_use]
pub fn is_pending_status(file_status: &str) -> bool {
    matches!(file_status, "pending_virus_scan" | "virus_scan_complete")
}

/// Human badge label for File status UI chrome.
#[must_use]
pub fn status_badge_label(file_status: &str) -> &'static str {
    match file_status {
        "available" => "Available",
        "pending_virus_scan" | "virus_scan_complete" => "Pending",
        "quarantined" => "Quarantined",
        _ => "Unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn available_emits_api_url() {
        assert_eq!(
            meson_image_src_for_status("profile_photo:abc", "available").as_deref(),
            Some("/api/files/abc")
        );
        assert_eq!(
            meson_image_src_for_status("abc", "available").as_deref(),
            Some("/api/files/abc")
        );
    }

    #[test]
    fn pending_and_quarantined_have_no_src() {
        assert!(meson_image_src_for_status("x:1", "pending_virus_scan").is_none());
        assert!(meson_image_src_for_status("x:1", "virus_scan_complete").is_none());
        assert!(meson_image_src_for_status("x:1", "quarantined").is_none());
    }

    #[test]
    fn badge_labels() {
        assert_eq!(status_badge_label("available"), "Available");
        assert_eq!(status_badge_label("pending_virus_scan"), "Pending");
        assert_eq!(status_badge_label("quarantined"), "Quarantined");
    }
}
