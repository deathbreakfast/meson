//! Boson task: build a large zip into the available store and tell the owner it's ready.

#![allow(missing_docs)]

use crate::blob_install::installed_blob_store;
use crate::events::publish_zip_ready;
use crate::zip::{build_archive, select_zip_rows, zip_object_key, ZipError, ZIP_LINK_TTL_HOURS};
use anyhow::Result;
use boson_core::ExecutionContext;
use boson_valence_identity::valence_from_context;
use serde::Serialize;
use valence::{RecordId, Valence};

/// Builds a zip too large for one request. Runs as the actor that asked for it.
#[boson_macros::task(
    name = "meson_zip_files",
    priority = 30,
    pool = "global",
    idempotency_mode = "lwt",
    max_attempts = 3,
    base_delay_ms = 5000,
    backoff_multiplier = 2.0,
    max_delay_ms = 60_000,
    max_in_flight = 4,
    max_enqueue_per_second = 20
)]
pub async fn meson_zip_files(
    ctx: Box<dyn ExecutionContext>,
    task_id: String,
    owner: String,
    file_ids: Vec<String>,
) -> Result<()> {
    let valence = valence_from_context(ctx.as_ref()).map_err(|e| anyhow::anyhow!("{e}"))?;
    let owner = RecordId::parse(&owner).ok_or_else(|| anyhow::anyhow!("bad owner id"))?;
    let ids: Vec<RecordId> = file_ids.iter().filter_map(|s| RecordId::parse(s)).collect();

    let selection = select_zip_rows(&valence, &owner, &ids)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let skipped = selection.skipped.len();
    let archive = build_archive(selection.rows)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let key = zip_object_key(&task_id);
    installed_blob_store()
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .put(&key, &archive.bytes)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let expires_at = chrono::Utc::now() + chrono::Duration::hours(ZIP_LINK_TTL_HOURS);
    tracing::info!(
        target: "meson.zip",
        outcome = "stored",
        files = archive.included.len(),
        skipped = skipped + archive.missing.len(),
        "background zip stored"
    );
    publish_zip_ready(
        owner.id(),
        &task_id,
        &key,
        expires_at,
        archive.included.len(),
        skipped + archive.missing.len(),
    )
    .await;
    Ok(())
}

#[derive(Serialize)]
struct ZipParams<'a> {
    task_id: &'a str,
    owner: String,
    file_ids: Vec<String>,
}

/// Enqueue `meson_zip_files` as `v`'s actor and return the new task id.
pub(crate) async fn enqueue_zip(
    v: &Valence,
    owner: &RecordId,
    ids: &[RecordId],
) -> Result<String, ZipError> {
    let boson = boson_runtime::default().ok_or(ZipError::BosonNotConfigured)?;
    let task_id = uuid::Uuid::new_v4().to_string();
    // The task runs as whoever asked for the zip, never as System.
    let actor_json =
        serde_json::to_value(v.actor()).map_err(|e| ZipError::Enqueue(e.to_string()))?;
    let params = ZipParams {
        task_id: &task_id,
        owner: owner.to_string(),
        file_ids: ids.iter().map(ToString::to_string).collect(),
    };
    let params_json = serde_json::to_value(params).map_err(|e| ZipError::Enqueue(e.to_string()))?;
    boson
        .enqueue(
            "meson_zip_files",
            actor_json,
            params_json,
            Some(format!("zip:{task_id}")),
        )
        .await
        .map_err(|e| ZipError::Enqueue(e.to_string()))?;
    Ok(task_id)
}
