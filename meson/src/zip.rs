//! Zip several of an owner's files into one download.
//!
//! [`zip_files`] keeps only the owner's Available files, skipping quarantined and still
//! scanning ones with their reason. Small selections come back as bytes. Above
//! [`ZIP_INLINE_MAX_FILES`] files or [`ZIP_INLINE_MAX_BYTES`] bytes, a Boson task
//! (`meson_zip_files`, feature `scan-boson`) builds the zip into the available store under
//! [`zip_object_key`] and publishes Photon `meson.zip.ready` (feature `photon`) with a link
//! expiry [`ZIP_LINK_TTL_HOURS`] hours out.

use crate::backend::FileStoreError;
use crate::blob_install::installed_blob_store;
use crate::generated::{FileFields, FileFileStatus, FileModel};
use crate::query_page::get_by_id;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use thiserror::Error;
use valence::{RecordId, Valence};

/// Most files [`zip_files`] zips in the request; more are zipped in the background.
pub const ZIP_INLINE_MAX_FILES: usize = 500;

/// Most bytes (by recorded `size_bytes`, 1 GB) [`zip_files`] zips in the request.
pub const ZIP_INLINE_MAX_BYTES: i64 = 1_000_000_000;

/// How long a background zip's download link is valid.
pub const ZIP_LINK_TTL_HOURS: i64 = 24;

/// Why [`zip_files`] left a file out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// No File row with that id belongs to the owner.
    NotFound,
    /// The virus scan found a problem; its bytes stay locked.
    Quarantined,
    /// The virus scan hasn't finished yet.
    Scanning,
    /// The row is Available but storage has no bytes for it.
    BlobMissing,
}

/// The zip itself, or the background task building it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZipOutput {
    /// The finished zip archive.
    Bytes(Vec<u8>),
    /// Too big for one request: the Boson task with this id builds it and publishes
    /// `meson.zip.ready` when it's stored.
    Queued {
        /// Boson task id; the stored object key is [`zip_object_key`] of it.
        task_id: String,
    },
}

/// Result of [`zip_files`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZipResult {
    /// Archive bytes or the background task id.
    pub output: ZipOutput,
    /// Files in the zip (or selected for the background zip).
    pub included: Vec<RecordId>,
    /// Files left out, each with its reason.
    pub skipped: Vec<(RecordId, SkipReason)>,
}

/// Errors from [`zip_files`].
#[derive(Debug, Error)]
pub enum ZipError {
    /// Valence failed a File lookup.
    #[error("valence: {0}")]
    Valence(#[from] valence::Error),
    /// The blob store is missing or failed a read or write.
    #[error("store: {0}")]
    Store(#[from] FileStoreError),
    /// Writing the zip archive failed.
    #[error("archive: {0}")]
    Archive(String),
    /// The selection needs a background zip and no Boson runtime is configured (or the
    /// crate was built without `scan-boson`).
    #[error("boson not configured")]
    BosonNotConfigured,
    /// Boson refused the background zip task.
    #[error("enqueue failed: {0}")]
    Enqueue(String),
}

/// Blob-store key a background zip is written to.
pub fn zip_object_key(task_id: &str) -> String {
    format!("zip-{task_id}.zip")
}

/// Zip `owner`'s files with these ids.
///
/// Each id is looked up owner-scoped, so someone else's id is skipped as
/// [`SkipReason::NotFound`]. Only Available files are read, from the available store.
/// Background zips report [`SkipReason::BlobMissing`] files in their Photon event rather
/// than here.
///
/// # Errors
///
/// [`ZipError`] when a lookup, a store read or the archive fails, or a background zip is
/// needed and can't be enqueued.
pub async fn zip_files(
    v: &Valence,
    owner: &RecordId,
    ids: &[RecordId],
) -> Result<ZipResult, ZipError> {
    let selection = select_zip_rows(v, owner, ids).await?;
    let total_bytes: i64 = selection.rows.iter().map(|row| *row.size_bytes()).sum();
    if selection.rows.len() > ZIP_INLINE_MAX_FILES || total_bytes > ZIP_INLINE_MAX_BYTES {
        let included: Vec<RecordId> = selection.rows.iter().filter_map(|r| r.id.clone()).collect();
        let task_id = enqueue_background_zip(v, owner, &included).await?;
        tracing::info!(
            target: "meson.zip",
            outcome = "queued",
            files = included.len(),
            "zip queued for the background"
        );
        return Ok(ZipResult {
            output: ZipOutput::Queued { task_id },
            included,
            skipped: selection.skipped,
        });
    }

    let mut skipped = selection.skipped;
    let archive = build_archive(selection.rows).await?;
    skipped.extend(
        archive
            .missing
            .into_iter()
            .map(|id| (id, SkipReason::BlobMissing)),
    );
    Ok(ZipResult {
        output: ZipOutput::Bytes(archive.bytes),
        included: archive.included,
        skipped,
    })
}

#[cfg(feature = "scan-boson")]
async fn enqueue_background_zip(
    v: &Valence,
    owner: &RecordId,
    ids: &[RecordId],
) -> Result<String, ZipError> {
    crate::jobs::enqueue_zip(v, owner, ids).await
}

#[cfg(not(feature = "scan-boson"))]
async fn enqueue_background_zip(
    _v: &Valence,
    _owner: &RecordId,
    _ids: &[RecordId],
) -> Result<String, ZipError> {
    Err(ZipError::BosonNotConfigured)
}

/// Owner's Available rows among `ids`, plus the ids left out and why.
pub(crate) struct ZipSelection {
    pub(crate) rows: Vec<FileModel>,
    pub(crate) skipped: Vec<(RecordId, SkipReason)>,
}

pub(crate) async fn select_zip_rows(
    v: &Valence,
    owner: &RecordId,
    ids: &[RecordId],
) -> valence::Result<ZipSelection> {
    let mut seen = HashSet::new();
    let mut rows = Vec::new();
    let mut skipped = Vec::new();
    for id in ids {
        if !seen.insert(id.clone()) {
            continue;
        }
        let purpose = valence::use_!(
            r"When someone downloads files from **My Files** as one zip, Meson loads **each chosen file's details** (name, size, scan status and storage location) to confirm it is **theirs**, to leave out files that are **quarantined or still being scanned**, and to read the bytes that go into the zip."
        );
        match get_by_id(v, purpose, owner, id).await? {
            None => skipped.push((id.clone(), SkipReason::NotFound)),
            Some(row) => match row.file_status() {
                FileFileStatus::Available => rows.push(row),
                FileFileStatus::Quarantined => skipped.push((id.clone(), SkipReason::Quarantined)),
                FileFileStatus::PendingVirusScan | FileFileStatus::VirusScanComplete => {
                    skipped.push((id.clone(), SkipReason::Scanning));
                }
            },
        }
    }
    Ok(ZipSelection { rows, skipped })
}

pub(crate) struct BuiltArchive {
    pub(crate) bytes: Vec<u8>,
    pub(crate) included: Vec<RecordId>,
    pub(crate) missing: Vec<RecordId>,
}

/// Read each row's bytes from the available store and write them into one zip.
pub(crate) async fn build_archive(rows: Vec<FileModel>) -> Result<BuiltArchive, ZipError> {
    let store = installed_blob_store()?;
    let mut writer = ::zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = ::zip::write::SimpleFileOptions::default()
        .compression_method(::zip::CompressionMethod::Deflated);
    let mut names = EntryNames::default();
    let mut included = Vec::new();
    let mut missing = Vec::new();
    for row in rows {
        let Some(id) = row.id.clone() else { continue };
        let bytes = match store.get(row.storage_path()).await {
            Ok(bytes) => bytes,
            Err(FileStoreError::NotFound) => {
                missing.push(id);
                continue;
            }
            Err(e) => return Err(e.into()),
        };
        writer
            .start_file(names.unique(row.file_name()), options)
            .map_err(|e| ZipError::Archive(e.to_string()))?;
        writer
            .write_all(&bytes)
            .map_err(|e| ZipError::Archive(e.to_string()))?;
        included.push(id);
    }
    let bytes = writer
        .finish()
        .map_err(|e| ZipError::Archive(e.to_string()))?
        .into_inner();
    Ok(BuiltArchive {
        bytes,
        included,
        missing,
    })
}

/// Safe, unique entry names: path separators replaced, repeats numbered ("a (2).png").
#[derive(Default)]
struct EntryNames {
    used: HashMap<String, usize>,
}

impl EntryNames {
    fn unique(&mut self, file_name: &str) -> String {
        let mut base: String = file_name
            .chars()
            .map(|c| {
                if matches!(c, '/' | '\\' | '\0') {
                    '_'
                } else {
                    c
                }
            })
            .collect();
        base = base.trim_start_matches('.').to_string();
        if base.is_empty() {
            base = "file".to_string();
        }
        let count = self.used.entry(base.clone()).or_insert(0);
        *count += 1;
        if *count == 1 {
            return base;
        }
        let n = *count;
        let name = match base.rsplit_once('.') {
            Some((stem, ext)) if !stem.is_empty() => format!("{stem} ({n}).{ext}"),
            _ => format!("{base} ({n})"),
        };
        // A numbered name can collide with a real file of that name; number again.
        if self.used.contains_key(&name) {
            return self.unique(&name);
        }
        self.used.insert(name.clone(), 1);
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_names_are_safe_and_unique() {
        let mut names = EntryNames::default();
        assert_eq!(names.unique("a.png"), "a.png");
        assert_eq!(names.unique("a.png"), "a (2).png");
        assert_eq!(names.unique("a.png"), "a (3).png");
        assert_eq!(names.unique("../etc/passwd"), "_etc_passwd");
        assert_eq!(names.unique(""), "file");
        assert_eq!(names.unique("README"), "README");
        assert_eq!(names.unique("README"), "README (2)");
    }

    #[test]
    fn zip_object_key_is_flat() {
        assert_eq!(zip_object_key("abc"), "zip-abc.zip");
    }
}
