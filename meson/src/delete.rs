//! Delete a File row with its bytes, let the owning app refuse, and name each File table's app.
//!
//! Each product File table opts in with [`FileApp`] (its display name and an optional
//! [`FileApp::before_delete`] check) and [`register_file_app!`](crate::register_file_app),
//! written next to the schema. [`delete_file`] then loads the caller's row, asks that hook,
//! deletes the row under the caller's actor and removes the bytes from the store that holds
//! them.

use crate::blob_install::{installed_blob_store, installed_quarantine_store};
use crate::generated::{FileFields, FileFileStatus, FileModel};
use crate::query_page::get_by_id;
use thiserror::Error;
use valence::privacy_policies::owner::check_owner_by_field;
use valence::{Actor, PrivacyRule, RecordId, Valence};

/// Why the app that stored a file refused to delete it ("attached to an open expense").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteRefusal {
    /// Sentence the app wants shown to the person who asked for the delete.
    pub reason: String,
}

impl DeleteRefusal {
    /// Refusal with `reason`.
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

/// A product File table's display name and delete check.
///
/// Implement it on the codegen model next to its `valence_schema!`, then call
/// [`register_file_app!`](crate::register_file_app) once for the type.
///
/// ```rust,ignore
/// impl meson::FileApp for FinanceReceiptFile {
///     const APP_NAME: &'static str = "Finance";
///
///     fn before_delete(row: &meson::generated::FileModel) -> Result<(), meson::DeleteRefusal> {
///         if receipt_is_on_open_expense(row) {
///             return Err(meson::DeleteRefusal::new("It's attached to an open expense."));
///         }
///         Ok(())
///     }
/// }
/// meson::register_file_app!(FinanceReceiptFile);
/// ```
pub trait FileApp: valence::Model + FileFields {
    /// Name people know the app by ("Finance", "Receipt scanner", "Profile").
    const APP_NAME: &'static str;

    /// Called by [`delete_file`] before anything is removed. Return a refusal to keep the
    /// row and its bytes. The default allows every delete.
    ///
    /// # Errors
    ///
    /// [`DeleteRefusal`] when the app needs the file kept.
    fn before_delete(row: &FileModel) -> Result<(), DeleteRefusal> {
        let _ = row;
        Ok(())
    }
}

/// Inventory entry written by [`register_file_app!`](crate::register_file_app).
#[doc(hidden)]
pub struct FileAppEntry {
    pub table: fn() -> &'static str,
    pub app_name: &'static str,
    pub before_delete: fn(&FileModel) -> Result<(), DeleteRefusal>,
}

valence::inventory::collect!(FileAppEntry);

/// Register a [`FileApp`] implementor so [`display_app_name`] and [`delete_file`] find it.
#[macro_export]
macro_rules! register_file_app {
    ($ty:ty) => {
        $crate::__private::inventory::submit! {
            $crate::__private::FileAppEntry {
                table: <$ty as $crate::__private::Model>::table_name,
                app_name: <$ty as $crate::FileApp>::APP_NAME,
                before_delete: <$ty as $crate::FileApp>::before_delete,
            }
        }
    };
}

fn file_app_entry(table: &str) -> Option<&'static FileAppEntry> {
    valence::inventory::iter::<FileAppEntry>
        .into_iter()
        .find(|entry| (entry.table)() == table)
}

/// Display name registered for a File table, if its app registered one.
pub fn display_app_name(table: &str) -> Option<&'static str> {
    file_app_entry(table).map(|entry| entry.app_name)
}

/// Display name for a File table, falling back to the table name when none is registered.
pub fn app_name_or_table(table: &str) -> &str {
    display_app_name(table).unwrap_or(table)
}

/// Privacy rule that allows the user named in the row's `uploaded_by` (and System).
///
/// Put it in a File table's `delete:` policy so people can delete their own uploads with
/// their own session actor:
///
/// ```rust,ignore
/// policies: {
///     delete: { allow: [meson::UPLOADED_BY_OWNER] },
/// },
/// ```
pub const UPLOADED_BY_OWNER: PrivacyRule = PrivacyRule {
    name: "meson_uploaded_by_owner",
    description: Some("Allow the user in uploaded_by"),
    check: uploaded_by_owner_check,
};

fn uploaded_by_owner_check(record: &serde_json::Value, viewer: &Actor) -> bool {
    check_owner_by_field(record, viewer, "uploaded_by")
}

/// What [`delete_file`] did with one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteOutcome {
    /// The row is gone. `blob_removed` is false when the bytes could not be removed; the
    /// failure is logged under target `meson.file_delete`.
    Deleted {
        /// Whether the stored bytes were removed too.
        blob_removed: bool,
    },
    /// No File row with that id belongs to the owner (missing, someone else's, or not a
    /// File table). Nothing changed.
    NotFound,
    /// The owning app refused. Nothing changed.
    Refused(DeleteRefusal),
}

/// Errors from [`delete_file`].
#[derive(Debug, Error)]
pub enum DeleteError {
    /// Valence refused or failed the lookup or the delete (for example the table's delete
    /// policy doesn't allow this actor). The row and its bytes are unchanged.
    #[error("valence: {0}")]
    Valence(#[from] valence::Error),
}

/// Delete one of `owner`'s files: its row, then its bytes.
///
/// Steps: load the row with [`get_by_id`] (owner-scoped), call the table's
/// [`FileApp::before_delete`], delete the row with the caller's actor (the table's
/// `delete:` policy decides), then remove the bytes from the available store (Available
/// rows) or the quarantine store (Pending and Quarantined rows). Quarantined files delete
/// like any other.
///
/// # Errors
///
/// [`DeleteError::Valence`] when the lookup or the row delete fails; the row and bytes are
/// left in place.
pub async fn delete_file(
    v: &Valence,
    owner: &RecordId,
    id: &RecordId,
) -> Result<DeleteOutcome, DeleteError> {
    let purpose = valence::use_!(
        r"When someone deletes a file from **My Files**, Meson loads **that one file's details** (name, type, size, scan status and storage location) to confirm it is **theirs**, to ask the app that stored it whether it may go, and to find the **stored bytes** it removes after the record."
    );
    let Some(row) = get_by_id(v, purpose, owner, id).await? else {
        return Ok(DeleteOutcome::NotFound);
    };

    if let Some(entry) = file_app_entry(id.table()) {
        if let Err(refusal) = (entry.before_delete)(&row) {
            tracing::info!(
                target: "meson.file_delete",
                table = %id.table(),
                outcome = "refused",
                "file delete refused by its app"
            );
            return Ok(DeleteOutcome::Refused(refusal));
        }
    }

    valence::delete_entity_now(id.table(), id.id(), v).await?;
    let blob_removed = remove_blob(&row).await;
    tracing::info!(
        target: "meson.file_delete",
        table = %id.table(),
        outcome = "deleted",
        blob_removed,
        "file deleted"
    );
    Ok(DeleteOutcome::Deleted { blob_removed })
}

/// [`delete_file`] for each id, in order, with one result per id.
pub async fn delete_files(
    v: &Valence,
    owner: &RecordId,
    ids: &[RecordId],
) -> Vec<(RecordId, Result<DeleteOutcome, DeleteError>)> {
    let mut results = Vec::with_capacity(ids.len());
    for id in ids {
        results.push((id.clone(), delete_file(v, owner, id).await));
    }
    results
}

async fn remove_blob(row: &FileModel) -> bool {
    let store = match row.file_status() {
        FileFileStatus::Available => installed_blob_store(),
        FileFileStatus::PendingVirusScan
        | FileFileStatus::VirusScanComplete
        | FileFileStatus::Quarantined => {
            installed_quarantine_store().or_else(|_| installed_blob_store())
        }
    };
    let result = match store {
        Ok(store) => store.delete(row.storage_path()).await,
        Err(e) => Err(e),
    };
    match result {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!(
                target: "meson.file_delete",
                outcome = "blob_left",
                error = %e,
                "file row deleted but its bytes were not removed"
            );
            false
        }
    }
}

impl FileApp for crate::generated::ReceiptScan {
    const APP_NAME: &'static str = "Receipt scanner";
}
crate::register_file_app!(crate::generated::ReceiptScan);

impl FileApp for crate::generated::E2eMesonProfilePhoto {
    const APP_NAME: &'static str = "Profile";
}
crate::register_file_app!(crate::generated::E2eMesonProfilePhoto);

impl FileApp for crate::generated::E2eMesonFile {
    const APP_NAME: &'static str = "Meson test files";
}
crate::register_file_app!(crate::generated::E2eMesonFile);

/// Test fixture: refuses to delete files whose name starts with `keep-`.
impl FileApp for crate::generated::E2eMesonOwnedFile {
    const APP_NAME: &'static str = "Meson owned test files";

    fn before_delete(row: &FileModel) -> Result<(), DeleteRefusal> {
        if row.file_name().starts_with("keep-") {
            return Err(DeleteRefusal::new("This test file is marked keep."));
        }
        Ok(())
    }
}
crate::register_file_app!(crate::generated::E2eMesonOwnedFile);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_names_cover_teaching_tables_and_fall_back() {
        assert_eq!(display_app_name("receipt_scan"), Some("Receipt scanner"));
        assert_eq!(display_app_name("e2e_meson_profile_photo"), Some("Profile"));
        assert_eq!(display_app_name("finance_receipt_file"), None);
        assert_eq!(
            app_name_or_table("finance_receipt_file"),
            "finance_receipt_file"
        );
    }

    #[test]
    fn uploaded_by_owner_allows_only_the_uploader() {
        let row = serde_json::json!({ "uploaded_by": "user:alice" });
        let alice = Actor::User {
            user_id: "alice".into(),
        };
        let bob = Actor::User {
            user_id: "bob".into(),
        };
        assert!((UPLOADED_BY_OWNER.check)(&row, &alice));
        assert!(!(UPLOADED_BY_OWNER.check)(&row, &bob));
        assert!(!(UPLOADED_BY_OWNER.check)(&row, &Actor::Anonymous));
    }
}
