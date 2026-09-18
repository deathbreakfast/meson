//! Session-scoped File selection helpers for owner-scoped file listings.

use valence::RecordId;

use crate::generated::{FileFields, FileModel};

/// True when `row.uploaded_by` equals the session user.
#[must_use]
pub fn is_uploaded_by(row: &impl FileFields, owner: &RecordId) -> bool {
    row.uploaded_by() == owner
}

/// Select the File row whose id matches `want` among already owner-filtered rows.
///
/// Returns `None` when the id is missing from the owner-scoped set. Callers treat
/// foreign ids and missing ids the same (not found) so a consumer never leaks
/// whether another user's file exists.
#[must_use]
pub fn find_file_in_owned_rows(
    rows: impl IntoIterator<Item = FileModel>,
    want: &RecordId,
) -> Option<FileModel> {
    rows.into_iter().find(|r| r.id.as_ref() == Some(want))
}
