//! Enqueue Boson `meson_virus_scan` after quarantine upload.

use serde::Serialize;
use thiserror::Error;

/// Errors enqueueing a virus scan.
#[derive(Debug, Error)]
pub enum EnqueueScanError {
    /// Boson runtime not configured.
    #[error("boson not configured")]
    BosonNotConfigured,
    /// Enqueue / serialize failure.
    #[error("enqueue failed")]
    Enqueue,
}

fn system_actor_json() -> serde_json::Value {
    serde_json::json!({"System": {"operation": "meson_virus_scan"}})
}

#[derive(Serialize)]
struct VirusScanParams<'a> {
    file_id: &'a str,
    table: &'a str,
}

/// Enqueue `meson_virus_scan` with LWT key `virus_scan:{table}:{file_id}`.
///
/// # Errors
///
/// [`EnqueueScanError`] when Boson is missing or enqueue fails.
pub async fn enqueue_virus_scan(table: &str, file_id: &str) -> Result<String, EnqueueScanError> {
    let boson = boson_runtime::default().ok_or(EnqueueScanError::BosonNotConfigured)?;
    let key = format!("virus_scan:{table}:{file_id}");
    let params = VirusScanParams { file_id, table };
    let params_json = serde_json::to_value(params).map_err(|_| EnqueueScanError::Enqueue)?;
    tracing::info!(
        target: "meson.virus_scan",
        operation = "enqueue",
        outcome = "queued",
        "meson virus scan enqueued"
    );
    boson
        .enqueue(
            "meson_virus_scan",
            system_actor_json(),
            params_json,
            Some(key),
        )
        .await
        .map_err(|_| EnqueueScanError::Enqueue)
}
