//! Synced image-src server functions for [`crate::MesonImg`].

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

/// Wire DTO: File status plus optional serve URL.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MesonImageSrc {
    /// File status wire string (`available`, `pending_virus_scan`, …).
    pub file_status: String,
    /// `/api/files/{id}` when Available; otherwise `None`.
    pub src: Option<String>,
}

/// Photon live tick for File status — registers `/ws/meson-files`.
///
/// Parameterized [`get_meson_image_src`] stays a normal read; clients key a
/// `Resource` on [`subscribe_meson_file_live_sync`] (hydrate hooks require a
/// zero-arg synced fn). Shares the `/ws/meson-files` topic with any other
/// consumer subscribing to File list updates.
#[cfg_attr(
    any(feature = "ssr", feature = "hydrate"),
    photon_leptos::synced(
        topic = "meson.file.updated",
        ws = "/ws/meson-files",
        strategy = "refetch",
        auth = "user"
    )
)]
#[server(MesonFileLiveSync)]
pub async fn meson_file_live_sync() -> Result<(), ServerFnError> {
    Ok(())
}

/// Resolve image src for a File id owned by the session user.
///
/// Returns status always (when the row is visible). `src` is set only for
/// `available`. Missing / foreign ids return `not_found` (IDOR collapse).
#[server(GetMesonImageSrc)]
pub async fn get_meson_image_src(file_id: String) -> Result<MesonImageSrc, ServerFnError> {
    #[cfg(feature = "ssr")]
    {
        resolve_image_src(&file_id)
            .await
            .map_err(ServerFnError::new)
    }
    #[cfg(not(feature = "ssr"))]
    {
        let _ = file_id;
        Err(ServerFnError::new("ssr required"))
    }
}

#[cfg(feature = "ssr")]
async fn resolve_image_src(file_id: &str) -> Result<MesonImageSrc, String> {
    use meson::find_file_in_owned_rows;
    use meson::generated::{FileFields, FileQueryAll};
    use valence::{RecordId, RecordPredicate};

    use crate::status::{bare_file_id, meson_image_src_for_status};

    let ctx = higgs::Higgs::from_request()
        .await
        .map_err(|e| format!("auth: {e}"))?;
    let user_raw = ctx
        .session_user_id()
        .ok_or_else(|| "auth: Authentication required".to_string())?;
    let user_rid =
        RecordId::parse(user_raw).ok_or_else(|| "auth: Invalid session user id".to_string())?;
    let v = ctx
        .valence()
        .map_err(|e| format!("auth: Failed to build Valence: {e}"))?;

    let rows = FileQueryAll::query_used(&v, valence::use_!(r#"In **Meson file storage**, we **list File Query All** so the product can show or process the matching set for this workflow. Callers allowed for **Meson file storage** use the list; it is not a public dump of every field to anonymous visitors."#))
        .where_uploaded_by(RecordPredicate::Equals(user_rid))
        .await
        .map_err(|e| format!("io: list files failed: {e}"))?;

    let row = match RecordId::parse(file_id) {
        Some(want) => find_file_in_owned_rows(rows, &want),
        None => {
            let bare = bare_file_id(file_id);
            rows.into_iter()
                .find(|r| r.id.as_ref().is_some_and(|rid| rid.id() == bare))
        }
    };

    let Some(row) = row else {
        tracing::debug!(
            target: "security",
            file_id = %file_id,
            outcome = "idor_collapse",
            "meson-leptos image src: owned File missing"
        );
        return Err("not_found: File not found".to_string());
    };

    let status = FileFields::file_status(&row).as_str().to_string();
    let serve_id = row
        .id
        .as_ref()
        .map(|rid| rid.id().to_string())
        .unwrap_or_else(|| bare_file_id(file_id).to_string());
    let src = meson_image_src_for_status(&serve_id, &status);

    Ok(MesonImageSrc {
        file_status: status,
        src,
    })
}

/// Subscribe helper when neither `ssr` nor `hydrate` is enabled.
#[cfg(not(any(feature = "ssr", feature = "hydrate")))]
pub fn subscribe_meson_file_live_sync(
    _on_event: impl Fn() + Send + Sync + 'static,
) -> RwSignal<u64> {
    RwSignal::new(0u64)
}
