//! Registry of table adapters that load/update File scan status for Boson workers.
//!
//! Products call [`register_file_scan_adapter`] at boot for each `File`-trait
//! table they own. Meson auto-registers teaching fixtures via
//! [`register_builtin_file_scan_adapters`].

use crate::generated::{
    E2eMesonFile, E2eMesonProfilePhoto, FileFields, FileFileStatus, ReceiptScan,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use thiserror::Error;
use valence::{Model, RecordId, Valence};

/// Snapshot of File fields needed by the virus-scan worker.
#[derive(Debug, Clone)]
pub struct FileScanSnapshot {
    /// Opaque object key.
    pub storage_path: String,
    /// Current scan status.
    pub file_status: FileFileStatus,
    /// Soft owner (`user` RecordId).
    pub uploaded_by: RecordId,
}

/// Errors from file-scan adapter operations.
#[derive(Debug, Error)]
pub enum FileScanAdapterError {
    /// No adapter registered for the table.
    #[error("no file scan adapter for table")]
    UnknownTable,
    /// Row missing.
    #[error("file not found")]
    NotFound,
    /// Valence load/update failed.
    #[error(transparent)]
    Valence(#[from] valence::Error),
}

/// Load storage_path/status/uploaded_by and commit Available / Quarantined.
#[async_trait]
pub trait FileScanAdapter: Send + Sync {
    /// Load a File row by bare id under `valence`.
    async fn load(
        &self,
        valence: &Valence,
        file_id: &str,
    ) -> Result<FileScanSnapshot, FileScanAdapterError>;

    /// Persist `Available` + optional new `storage_path` after promote.
    async fn commit_available(
        &self,
        valence: &Valence,
        file_id: &str,
        storage_path: String,
    ) -> Result<(), FileScanAdapterError>;

    /// Persist `Quarantined` (bytes remain in quarantine store).
    async fn commit_quarantined(
        &self,
        valence: &Valence,
        file_id: &str,
    ) -> Result<(), FileScanAdapterError>;
}

fn registry() -> &'static Mutex<HashMap<&'static str, Arc<dyn FileScanAdapter>>> {
    static REG: OnceLock<Mutex<HashMap<&'static str, Arc<dyn FileScanAdapter>>>> = OnceLock::new();
    REG.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register (or replace) an adapter for `table`.
pub fn register_file_scan_adapter(table: &'static str, adapter: Arc<dyn FileScanAdapter>) {
    let mut guard = registry()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    guard.insert(table, adapter);
}

/// Look up an adapter for `table`.
///
/// # Errors
///
/// [`FileScanAdapterError::UnknownTable`] when none is registered.
pub fn file_scan_adapter(table: &str) -> Result<Arc<dyn FileScanAdapter>, FileScanAdapterError> {
    ensure_builtins_registered();
    let guard = registry()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    guard
        .get(table)
        .cloned()
        .ok_or(FileScanAdapterError::UnknownTable)
}

/// Clear the registry (tests only).
pub fn clear_file_scan_adapters_for_test() {
    let mut guard = registry()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    guard.clear();
    let mut flag = builtins_flag()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *flag = None;
}

/// Load a File scan snapshot via the registered adapter.
///
/// # Errors
///
/// Unknown table or adapter errors.
pub async fn load_file_for_scan(
    valence: &Valence,
    table: &str,
    file_id: &str,
) -> Result<FileScanSnapshot, FileScanAdapterError> {
    file_scan_adapter(table)?.load(valence, file_id).await
}

/// Commit Available after promote.
///
/// # Errors
///
/// Unknown table or adapter errors.
pub async fn commit_file_available(
    valence: &Valence,
    table: &str,
    file_id: &str,
    storage_path: String,
) -> Result<(), FileScanAdapterError> {
    file_scan_adapter(table)?
        .commit_available(valence, file_id, storage_path)
        .await
}

/// Commit Quarantined.
///
/// # Errors
///
/// Unknown table or adapter errors.
pub async fn commit_file_quarantined(
    valence: &Valence,
    table: &str,
    file_id: &str,
) -> Result<(), FileScanAdapterError> {
    file_scan_adapter(table)?
        .commit_quarantined(valence, file_id)
        .await
}

static BUILTINS: OnceLock<Mutex<Option<()>>> = OnceLock::new();

fn builtins_flag() -> &'static Mutex<Option<()>> {
    BUILTINS.get_or_init(|| Mutex::new(None))
}

fn ensure_builtins_registered() {
    let mut guard = builtins_flag()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if guard.is_some() {
        return;
    }
    register_builtin_file_scan_adapters();
    *guard = Some(());
}

/// Register Meson teaching File models (`e2e_meson_file`, `e2e_meson_profile_photo`,
/// `receipt_scan`).
pub fn register_builtin_file_scan_adapters() {
    register_file_scan_adapter(
        "e2e_meson_file",
        Arc::new(ModelFileScanAdapter::<E2eMesonFile>::default()),
    );
    register_file_scan_adapter(
        "e2e_meson_profile_photo",
        Arc::new(ModelFileScanAdapter::<E2eMesonProfilePhoto>::default()),
    );
    register_file_scan_adapter(
        "receipt_scan",
        Arc::new(ModelFileScanAdapter::<ReceiptScan>::default()),
    );
}

/// Generic adapter over generated File models with Mutable builders.
pub struct ModelFileScanAdapter<M> {
    _marker: std::marker::PhantomData<M>,
}

impl<M> Default for ModelFileScanAdapter<M> {
    fn default() -> Self {
        Self {
            _marker: std::marker::PhantomData,
        }
    }
}

/// Trait alias for models that support scan status commits via Mutable.
pub trait FileScanModel: Model + FileFields + Clone + Send + Sync + 'static {
    /// Mutable commit Available + storage_path.
    fn commit_available_status<'a>(
        model: Self,
        valence: &'a Valence,
        storage_path: String,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), FileScanAdapterError>> + Send + 'a>,
    >;

    /// Mutable commit Quarantined.
    fn commit_quarantined_status<'a>(
        model: Self,
        valence: &'a Valence,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<(), FileScanAdapterError>> + Send + 'a>,
    >;
}

macro_rules! impl_file_scan_model {
    ($ty:ty) => {
        impl FileScanModel for $ty {
            fn commit_available_status<'a>(
                model: Self,
                valence: &'a Valence,
                storage_path: String,
            ) -> std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<(), FileScanAdapterError>> + Send + 'a>,
            > {
                Box::pin(async move {
                    model
                        .get_mutable(valence, valence::use_!(r"In **Meson file storage**, we **update this data** so later steps see the latest values for this workflow. Callers allowed for **Meson file storage** use the updated data; this is not a public export of unrelated fields."))
                        .set_storage_path(storage_path)
                        .map_err(FileScanAdapterError::Valence)?
                        .set_file_status(FileFileStatus::Available)
                        .map_err(FileScanAdapterError::Valence)?
                        .commit()
                        .await
                        .map_err(FileScanAdapterError::Valence)?;
                    Ok(())
                })
            }

            fn commit_quarantined_status<'a>(
                model: Self,
                valence: &'a Valence,
            ) -> std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<(), FileScanAdapterError>> + Send + 'a>,
            > {
                Box::pin(async move {
                    model
                        .get_mutable(valence, valence::use_!(r"In **Meson file storage**, we **update this data** so later steps see the latest values for this workflow. Callers allowed for **Meson file storage** use the updated data; this is not a public export of unrelated fields."))
                        .set_file_status(FileFileStatus::Quarantined)
                        .map_err(FileScanAdapterError::Valence)?
                        .commit()
                        .await
                        .map_err(FileScanAdapterError::Valence)?;
                    Ok(())
                })
            }
        }
    };
}

impl_file_scan_model!(E2eMesonFile);
impl_file_scan_model!(E2eMesonProfilePhoto);
impl_file_scan_model!(ReceiptScan);

#[async_trait]
impl<M: FileScanModel> FileScanAdapter for ModelFileScanAdapter<M> {
    async fn load(
        &self,
        valence: &Valence,
        file_id: &str,
    ) -> Result<FileScanSnapshot, FileScanAdapterError> {
        let row = M::get(file_id, valence, valence::use_!(r"In **Meson file storage**, we **load M** so the application can decide what to do next in this workflow. The result is used by **Meson file storage** logic—not necessarily displayed on a page unless that feature’s UI shows it."))
            .await?
            .ok_or(FileScanAdapterError::NotFound)?;
        Ok(FileScanSnapshot {
            storage_path: FileFields::storage_path(&row).clone(),
            file_status: FileFields::file_status(&row).clone(),
            uploaded_by: FileFields::uploaded_by(&row).clone(),
        })
    }

    async fn commit_available(
        &self,
        valence: &Valence,
        file_id: &str,
        storage_path: String,
    ) -> Result<(), FileScanAdapterError> {
        let row = M::get(file_id, valence, valence::use_!(r"In **Meson file storage**, we **load M** so the application can decide what to do next in this workflow. The result is used by **Meson file storage** logic—not necessarily displayed on a page unless that feature’s UI shows it."))
            .await?
            .ok_or(FileScanAdapterError::NotFound)?;
        M::commit_available_status(row, valence, storage_path).await
    }

    async fn commit_quarantined(
        &self,
        valence: &Valence,
        file_id: &str,
    ) -> Result<(), FileScanAdapterError> {
        let row = M::get(file_id, valence, valence::use_!(r"In **Meson file storage**, we **load M** so the application can decide what to do next in this workflow. The result is used by **Meson file storage** logic—not necessarily displayed on a page unless that feature’s UI shows it."))
            .await?
            .ok_or(FileScanAdapterError::NotFound)?;
        M::commit_quarantined_status(row, valence).await
    }
}
