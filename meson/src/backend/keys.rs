//! Shared flat-object-key validation for all [`FileByteBackend`](super::FileByteBackend) impls.

use super::FileStoreError;
use std::path::Path;

/// Reject empty, absolute, `..`, `/`, and `\` keys (flat object keys only).
pub(crate) fn validate_object_key(key: &str) -> Result<(), FileStoreError> {
    if key.is_empty()
        || key.contains("..")
        || key.contains('/')
        || key.contains('\\')
        || Path::new(key).is_absolute()
    {
        return Err(FileStoreError::InvalidKey);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_flat_key() {
        assert!(validate_object_key("a.png").is_ok());
        assert!(validate_object_key("uuid-here.bin").is_ok());
    }

    #[test]
    fn rejects_escape_and_empty() {
        assert!(matches!(
            validate_object_key(""),
            Err(FileStoreError::InvalidKey)
        ));
        assert!(matches!(
            validate_object_key("../x.png"),
            Err(FileStoreError::InvalidKey)
        ));
        assert!(matches!(
            validate_object_key("a/b.png"),
            Err(FileStoreError::InvalidKey)
        ));
        assert!(matches!(
            validate_object_key("a\\b.png"),
            Err(FileStoreError::InvalidKey)
        ));
        assert!(matches!(
            validate_object_key("/abs.png"),
            Err(FileStoreError::InvalidKey)
        ));
    }
}
