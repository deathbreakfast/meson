//! Mime-class helpers for File preview routing.

/// How a consumer should render a File payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewKind {
    /// `image/*` — Orbital `Image`.
    Image,
    /// Text-ish payloads — Orbital `Code`.
    Text,
    /// No in-app preview; offer download only.
    Unsupported,
}

/// Classify a stored `mime_type` for preview.
#[must_use]
pub fn preview_kind(mime_type: &str) -> PreviewKind {
    let mime = mime_type.trim().to_ascii_lowercase();
    if mime.starts_with("image/") {
        return PreviewKind::Image;
    }
    if mime.starts_with("text/")
        || mime == "application/json"
        || mime == "application/xml"
        || mime == "application/csv"
        || mime == "text/csv"
        || mime.ends_with("+json")
        || mime.ends_with("+xml")
    {
        return PreviewKind::Text;
    }
    // Common CSV upload labels
    if mime == "application/vnd.ms-excel" {
        return PreviewKind::Text;
    }
    PreviewKind::Unsupported
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_image_text_unsupported() {
        assert_eq!(preview_kind("image/png"), PreviewKind::Image);
        assert_eq!(preview_kind("TEXT/PLAIN"), PreviewKind::Text);
        assert_eq!(preview_kind("application/json"), PreviewKind::Text);
        assert_eq!(preview_kind("application/pdf"), PreviewKind::Unsupported);
        assert_eq!(preview_kind("application/zip"), PreviewKind::Unsupported);
    }
}
