//! Boson task: scan quarantine bytes and promote or quarantine the File row.

use crate::blob_install::installed_quarantine_store;
use crate::file_scan_adapter::{
    commit_file_available, commit_file_quarantined, load_file_for_scan,
};
use crate::generated::FileFileStatus;
use crate::promote::promote_to_available;
use crate::scan::ScanVerdict;
use anyhow::Result;
use boson_core::ExecutionContext;
use boson_valence_identity::valence_from_context;

/// Durable virus scan for a quarantine File row.
#[boson_macros::task(
    name = "meson_virus_scan",
    priority = 40,
    pool = "global",
    idempotency_mode = "lwt",
    max_attempts = 5,
    base_delay_ms = 1000,
    backoff_multiplier = 2.0,
    max_delay_ms = 60_000,
    max_in_flight = 50,
    max_enqueue_per_second = 100
)]
pub async fn meson_virus_scan(
    ctx: Box<dyn ExecutionContext>,
    file_id: String,
    table: String,
) -> Result<()> {
    let valence = valence_from_context(ctx.as_ref()).map_err(|e| anyhow::anyhow!("{e}"))?;
    let view = load_file_for_scan(&valence, &table, &file_id)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    match view.file_status {
        FileFileStatus::Available | FileFileStatus::Quarantined => {
            tracing::info!(
                target: "meson.virus_scan",
                operation = "meson_virus_scan",
                outcome = "already_terminal",
                "skip scan; file already terminal"
            );
            return Ok(());
        }
        FileFileStatus::PendingVirusScan | FileFileStatus::VirusScanComplete => {}
    }

    let started = std::time::Instant::now();
    let result = scan_and_commit(&valence, &table, &file_id, &view).await;
    let scan_ms = started.elapsed().as_millis();
    let pending_ms = (chrono::Utc::now() - view.uploaded_at).num_milliseconds();
    match result {
        Ok(status) => {
            let outcome = if matches!(status, FileFileStatus::Available) {
                "clean"
            } else {
                "infected"
            };
            tracing::info!(
                target: "meson.virus_scan",
                operation = "meson_virus_scan",
                outcome,
                verdict = outcome,
                table = %table,
                file_id = %file_id,
                scan_ms,
                pending_ms,
                "virus scan finished"
            );
            Ok(())
        }
        Err((reason_class, err)) => {
            tracing::warn!(
                target: "meson.virus_scan",
                operation = "meson_virus_scan",
                outcome = "scan_error",
                reason_class,
                table = %table,
                file_id = %file_id,
                scan_ms,
                pending_ms,
                error = %err,
                "virus scan attempt failed"
            );
            Err(err)
        }
    }
}

/// Scan the quarantine bytes and commit the verdict. The error carries the stage
/// that failed as a `reason_class`.
async fn scan_and_commit(
    valence: &valence::Valence,
    table: &str,
    file_id: &str,
    view: &crate::file_scan_adapter::FileScanSnapshot,
) -> Result<FileFileStatus, (&'static str, anyhow::Error)> {
    let stage =
        |class: &'static str| move |e: &dyn std::fmt::Display| (class, anyhow::anyhow!("{e}"));
    let quarantine = installed_quarantine_store().map_err(|e| stage("quarantine_store")(&e))?;
    let bytes = quarantine
        .get(&view.storage_path)
        .await
        .map_err(|e| stage("quarantine_read")(&e))?;
    let verdict = crate::scan::resolve_virus_scanner()
        .scan(&bytes)
        .await
        .map_err(|e| stage("scan")(&e))?;

    let status = match verdict {
        ScanVerdict::Clean => {
            let new_path = promote_to_available(&view.storage_path)
                .await
                .map_err(|e| stage("promote")(&e))?;
            commit_file_available(valence, table, file_id, new_path)
                .await
                .map_err(|e| stage("commit")(&e))?;
            FileFileStatus::Available
        }
        ScanVerdict::Infected { .. } => {
            commit_file_quarantined(valence, table, file_id)
                .await
                .map_err(|e| stage("commit")(&e))?;
            FileFileStatus::Quarantined
        }
    };
    crate::events::publish_file_updated(
        &photon_user_key(&view.uploaded_by),
        file_id,
        status.as_str(),
    )
    .await;
    Ok(status)
}

fn photon_user_key(rid: &valence::RecordId) -> String {
    // Photon auth keys are the full `table:id` form, not a stripped bare uuid.
    rid.to_string()
}
