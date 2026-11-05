//! Chronon sweeper for stuck `PendingVirusScan` rows.

use anyhow::Result;

/// Re-enqueue aged pending virus scans. The default job runs every five minutes.
///
/// Payload: optional `max_age_secs` (default 300). Scans FileQueryAll for
/// `pending_virus_scan` and enqueues `meson_virus_scan` again.
#[chronon_coordinator_macros::script(
    name = "meson_virus_scan_sweeper",
    default_job(job = "meson-virus-scan-sweeper", cron = "*/5 * * * *")
)]
pub async fn meson_virus_scan_sweeper(
    ctx: Box<dyn chronon_core::ScriptContext>,
    max_age_secs: Option<i64>,
) -> Result<()> {
    let max_age = max_age_secs.unwrap_or(300);
    let valence = chronon_valence_identity::valence_from_context(&*ctx)?;
    use crate::generated::{FileFields, FileFileStatus, FileQueryAll};
    use valence::StringPredicate;

    let rows = FileQueryAll::query(&valence, valence::use_!(r"In **Meson file storage**, we **list File Query All** so the product can show or process the matching set for this workflow. Callers allowed for **Meson file storage** use the list; it is not a public dump of every field to anonymous visitors."))
        .where_file_status(StringPredicate::Equals(
            FileFileStatus::PendingVirusScan.as_str().to_string(),
        ))
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let now = chrono::Utc::now();
    let pending_total = rows.len();
    let mut stale = 0u32;
    let mut oldest_pending_secs = 0i64;
    let mut enqueued = 0u32;
    for row in rows {
        let uploaded_at = *FileFields::uploaded_at(&row);
        if is_stale(uploaded_at, now, max_age) {
            stale += 1;
            oldest_pending_secs = oldest_pending_secs.max((now - uploaded_at).num_seconds());
            // FileModel exposes `id` as a public field (Option<RecordId>).
            if let Some(ref rid) = row.id {
                let table = rid.table().to_string();
                let bare = rid.id().to_string();
                #[cfg(feature = "scan-boson")]
                {
                    match crate::enqueue::enqueue_virus_scan(&table, &bare).await {
                        Ok(_) => enqueued += 1,
                        Err(e) => tracing::warn!(
                            target: "meson.virus_scan",
                            operation = "sweeper",
                            outcome = "enqueue_failed",
                            error = %e,
                            "sweeper enqueue failed"
                        ),
                    }
                }
                #[cfg(not(feature = "scan-boson"))]
                {
                    let _ = (table, bare);
                }
            }
        }
    }
    tracing::info!(
        target: "meson.virus_scan",
        operation = "sweeper",
        outcome = "ok",
        enqueued,
        pending_total,
        stale,
        oldest_pending_secs,
        "virus scan sweeper finished"
    );
    Ok(())
}

/// True when a row uploaded at `uploaded_at` has been pending longer than `max_age_secs`.
fn is_stale(
    uploaded_at: chrono::DateTime<chrono::Utc>,
    now: chrono::DateTime<chrono::Utc>,
    max_age_secs: i64,
) -> bool {
    uploaded_at < now - chrono::Duration::seconds(max_age_secs)
}

#[cfg(test)]
mod tests {
    use super::is_stale;
    use chrono::{Duration, Utc};

    #[test]
    fn fresh_row_is_not_stale() {
        let now = Utc::now();
        assert!(!is_stale(now - Duration::seconds(10), now, 300));
    }

    #[test]
    fn old_row_is_stale() {
        let now = Utc::now();
        assert!(is_stale(now - Duration::seconds(301), now, 300));
    }

    #[test]
    fn row_exactly_at_cutoff_is_not_stale() {
        let now = Utc::now();
        assert!(!is_stale(now - Duration::seconds(300), now, 300));
    }
}
