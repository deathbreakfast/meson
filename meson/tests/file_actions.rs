//! Owner-scoped file actions: get by id, type-ahead, delete with the app hook, zip.
//!
//! Scenario IDs: `meson-get-by-id`, `meson-suggest-names`, `meson-delete-*`,
//! `meson-zip-inline`, `meson-file-indexes`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

mod support;

use std::io::Read;
use std::sync::Arc;

use meson::generated::{FileFields, FileFileStatus};
use meson::{
    app_name_or_table, clear_blob_stores_for_test, delete_file, delete_files, display_app_name,
    get_by_id, install_blob_store, install_quarantine_store, suggest_names, zip_files, DeleteError,
    DeleteOutcome, FileByteBackend, FileStoreError, LocalDiskBlobStore, SkipReason, ZipOutput,
};
use support::{
    as_user, blob_install_lock, owner_rid, peer_rid, seed_file, seed_owned_file, setup_valence,
    OWNER_USER_ID, PEER_USER_ID,
};
use valence::{RecordId, Valence};

const OWNED: &str = "e2e_meson_owned_file";

struct Stores {
    available: Arc<LocalDiskBlobStore>,
    quarantine: Arc<LocalDiskBlobStore>,
}

fn temp_root(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "meson-actions-{label}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn install_stores() -> Stores {
    clear_blob_stores_for_test();
    let available = Arc::new(LocalDiskBlobStore::new(temp_root("a")));
    let quarantine = Arc::new(LocalDiskBlobStore::new(temp_root("q")));
    install_blob_store(available.clone()).unwrap();
    install_quarantine_store(quarantine.clone()).unwrap();
    Stores {
        available,
        quarantine,
    }
}

fn purpose() -> valence::DataUsePurpose {
    valence::use_!(
        r"**Test:** Owner-scoped **File** lookup in `file_actions` so the suite can check get by id and type-ahead."
    )
}

async fn owned_available(system: &Valence, stores: &Stores, id: &str, name: &str) -> RecordId {
    let path = format!("{id}.bin");
    stores.available.put(&path, name.as_bytes()).await.unwrap();
    seed_owned_file(
        system,
        id,
        owner_rid(),
        name,
        &path,
        10,
        FileFileStatus::Available,
    )
    .await
}

#[tokio::test]
async fn meson_get_by_id_is_owner_scoped() {
    let system = setup_valence().await;
    let rid = seed_owned_file(
        &system,
        "g1",
        owner_rid(),
        "mine.png",
        "g1.png",
        12,
        FileFileStatus::Available,
    )
    .await;
    let owner_v = as_user(&system, OWNER_USER_ID);
    let peer_v = as_user(&system, PEER_USER_ID);

    let row = get_by_id(&owner_v, purpose(), &owner_rid(), &rid)
        .await
        .unwrap()
        .expect("owner sees own file");
    assert_eq!(row.file_name(), "mine.png");
    assert_eq!(row.id.as_ref(), Some(&rid));

    assert!(get_by_id(&peer_v, purpose(), &peer_rid(), &rid)
        .await
        .unwrap()
        .is_none());
    assert!(
        get_by_id(
            &owner_v,
            purpose(),
            &owner_rid(),
            &RecordId::new("user", "g1")
        )
        .await
        .unwrap()
        .is_none(),
        "a non-File table answers none"
    );
    assert!(get_by_id(
        &owner_v,
        purpose(),
        &owner_rid(),
        &RecordId::new(OWNED, "missing")
    )
    .await
    .unwrap()
    .is_none());
}

#[tokio::test]
async fn meson_suggest_names_prefix_cap_and_owner() {
    let system = setup_valence().await;
    for i in 1..=12 {
        seed_owned_file(
            &system,
            &format!("s{i}"),
            owner_rid(),
            &format!("rec-{i:02}.png"),
            &format!("s{i}.png"),
            1,
            FileFileStatus::Available,
        )
        .await;
    }
    seed_owned_file(
        &system,
        "s-other",
        owner_rid(),
        "other.png",
        "o.png",
        1,
        FileFileStatus::Available,
    )
    .await;
    seed_owned_file(
        &system,
        "s-peer",
        peer_rid(),
        "rec-peer.png",
        "p.png",
        1,
        FileFileStatus::Available,
    )
    .await;
    // Same name in a second File table is suggested once.
    seed_file(
        &system,
        "s-dup",
        owner_rid(),
        "rec-01.png",
        "image/png",
        "d.png",
        1,
    )
    .await;

    let owner_v = as_user(&system, OWNER_USER_ID);
    let names = suggest_names(&owner_v, purpose(), &owner_rid(), "rec", 25)
        .await
        .unwrap();
    assert_eq!(names.len(), 10, "capped at 10: {names:?}");
    assert!(names
        .iter()
        .all(|n| n.starts_with("rec-") && n != "rec-peer.png"));
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted);
    assert_eq!(names.iter().filter(|n| *n == "rec-01.png").count(), 1);

    assert_eq!(
        suggest_names(&owner_v, purpose(), &owner_rid(), "", 5)
            .await
            .unwrap(),
        Vec::<String>::new()
    );
}

#[tokio::test]
async fn meson_delete_removes_row_and_blob() {
    let _g = blob_install_lock().await;
    let stores = install_stores();
    let system = setup_valence().await;
    let rid = owned_available(&system, &stores, "d1", "gone.png").await;
    let owner_v = as_user(&system, OWNER_USER_ID);

    let outcome = delete_file(&owner_v, &owner_rid(), &rid).await.unwrap();
    assert_eq!(outcome, DeleteOutcome::Deleted { blob_removed: true });
    assert!(get_by_id(&system, purpose(), &owner_rid(), &rid)
        .await
        .unwrap()
        .is_none());
    assert!(matches!(
        stores.available.get("d1.bin").await,
        Err(FileStoreError::NotFound)
    ));
}

#[tokio::test]
async fn meson_delete_refused_by_app_keeps_row_and_blob() {
    let _g = blob_install_lock().await;
    let stores = install_stores();
    let system = setup_valence().await;
    let rid = owned_available(&system, &stores, "d2", "keep-me.png").await;
    let owner_v = as_user(&system, OWNER_USER_ID);

    let outcome = delete_file(&owner_v, &owner_rid(), &rid).await.unwrap();
    let DeleteOutcome::Refused(refusal) = outcome else {
        panic!("expected refusal, got {outcome:?}");
    };
    assert_ne!(refusal.reason, "");
    assert!(get_by_id(&system, purpose(), &owner_rid(), &rid)
        .await
        .unwrap()
        .is_some());
    assert_eq!(
        stores.available.get("d2.bin").await.unwrap(),
        b"keep-me.png"
    );
}

#[tokio::test]
async fn meson_delete_denied_by_policy_keeps_row_and_blob() {
    let _g = blob_install_lock().await;
    let stores = install_stores();
    let system = setup_valence().await;
    // e2e_meson_file's delete policy is SYSTEM_ONLY.
    stores.available.put("d3.png", b"x").await.unwrap();
    seed_file(
        &system,
        "d3",
        owner_rid(),
        "locked.png",
        "image/png",
        "d3.png",
        1,
    )
    .await;
    let rid = RecordId::new("e2e_meson_file", "d3");
    let owner_v = as_user(&system, OWNER_USER_ID);

    let err = delete_file(&owner_v, &owner_rid(), &rid).await.unwrap_err();
    assert!(matches!(err, DeleteError::Valence(_)));
    assert!(get_by_id(&system, purpose(), &owner_rid(), &rid)
        .await
        .unwrap()
        .is_some());
    assert_eq!(stores.available.get("d3.png").await.unwrap(), b"x");
}

#[tokio::test]
async fn meson_delete_quarantined_removes_quarantine_bytes() {
    let _g = blob_install_lock().await;
    let stores = install_stores();
    let system = setup_valence().await;
    stores.quarantine.put("d4.bin", b"bad").await.unwrap();
    let rid = seed_owned_file(
        &system,
        "d4",
        owner_rid(),
        "bad.exe",
        "d4.bin",
        3,
        FileFileStatus::Quarantined,
    )
    .await;
    let owner_v = as_user(&system, OWNER_USER_ID);

    let outcome = delete_file(&owner_v, &owner_rid(), &rid).await.unwrap();
    assert_eq!(outcome, DeleteOutcome::Deleted { blob_removed: true });
    assert!(matches!(
        stores.quarantine.get("d4.bin").await,
        Err(FileStoreError::NotFound)
    ));
}

#[tokio::test]
async fn meson_delete_files_reports_each_id() {
    let _g = blob_install_lock().await;
    let stores = install_stores();
    let system = setup_valence().await;
    let mine = owned_available(&system, &stores, "b1", "a.png").await;
    let kept = owned_available(&system, &stores, "b2", "keep-b.png").await;
    let theirs = seed_owned_file(
        &system,
        "b3",
        peer_rid(),
        "t.png",
        "b3.bin",
        1,
        FileFileStatus::Available,
    )
    .await;
    let owner_v = as_user(&system, OWNER_USER_ID);

    let results = delete_files(
        &owner_v,
        &owner_rid(),
        &[mine.clone(), theirs.clone(), kept.clone()],
    )
    .await;
    assert_eq!(results.len(), 3);
    assert_eq!(results[0].0, mine);
    assert!(matches!(results[0].1, Ok(DeleteOutcome::Deleted { .. })));
    assert_eq!(results[1].0, theirs);
    assert!(matches!(results[1].1, Ok(DeleteOutcome::NotFound)));
    assert_eq!(results[2].0, kept);
    assert!(matches!(results[2].1, Ok(DeleteOutcome::Refused(_))));
    assert!(get_by_id(&system, purpose(), &peer_rid(), &theirs)
        .await
        .unwrap()
        .is_some());
}

#[tokio::test]
async fn meson_zip_inline_skips_quarantined_scanning_and_foreign() {
    let _g = blob_install_lock().await;
    let stores = install_stores();
    let system = setup_valence().await;
    let a = owned_available(&system, &stores, "z1", "a.png").await;
    let a2 = owned_available(&system, &stores, "z2", "a.png").await;
    let quarantined = seed_owned_file(
        &system,
        "z3",
        owner_rid(),
        "q.png",
        "z3.bin",
        1,
        FileFileStatus::Quarantined,
    )
    .await;
    let scanning = seed_owned_file(
        &system,
        "z4",
        owner_rid(),
        "p.png",
        "z4.bin",
        1,
        FileFileStatus::PendingVirusScan,
    )
    .await;
    let foreign = seed_owned_file(
        &system,
        "z5",
        peer_rid(),
        "f.png",
        "z5.bin",
        1,
        FileFileStatus::Available,
    )
    .await;
    let no_bytes = seed_owned_file(
        &system,
        "z6",
        owner_rid(),
        "gone.png",
        "z6.bin",
        1,
        FileFileStatus::Available,
    )
    .await;
    let owner_v = as_user(&system, OWNER_USER_ID);

    let ids = [
        a.clone(),
        a2.clone(),
        quarantined.clone(),
        scanning.clone(),
        foreign.clone(),
        no_bytes.clone(),
    ];
    let result = zip_files(&owner_v, &owner_rid(), &ids).await.unwrap();
    assert_eq!(result.included, vec![a, a2]);
    assert_eq!(
        result.skipped,
        vec![
            (quarantined, SkipReason::Quarantined),
            (scanning, SkipReason::Scanning),
            (foreign, SkipReason::NotFound),
            (no_bytes, SkipReason::BlobMissing),
        ]
    );
    let ZipOutput::Bytes(bytes) = result.output else {
        panic!("expected inline bytes");
    };
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut names: Vec<String> = archive
        .file_names()
        .map(|n| n.unwrap().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, vec!["a (2).png".to_string(), "a.png".to_string()]);
    let mut body = String::new();
    archive
        .by_name("a.png")
        .unwrap()
        .read_to_string(&mut body)
        .unwrap();
    assert_eq!(body, "a.png");
}

#[cfg(not(feature = "scan-boson"))]
#[tokio::test]
async fn meson_zip_too_big_without_boson_errors() {
    let _g = blob_install_lock().await;
    let _stores = install_stores();
    let system = setup_valence().await;
    let big = seed_owned_file(
        &system,
        "zb",
        owner_rid(),
        "big.iso",
        "zb.bin",
        2_000_000_000,
        FileFileStatus::Available,
    )
    .await;
    let owner_v = as_user(&system, OWNER_USER_ID);
    let err = zip_files(&owner_v, &owner_rid(), &[big]).await.unwrap_err();
    assert!(matches!(err, meson::ZipError::BosonNotConfigured));
}

#[test]
fn meson_app_names_registered_next_to_schema() {
    assert_eq!(display_app_name(OWNED), Some("Meson owned test files"));
    assert_eq!(display_app_name("receipt_scan"), Some("Receipt scanner"));
    assert_eq!(app_name_or_table("unknown_files"), "unknown_files");
}

/// Typed-table sync emits `CREATE INDEX IF NOT EXISTS` for every declared `indexed`
/// field. SQLite's live layout inspection doesn't report index flags, so this checks the
/// declared schema each File table syncs from.
#[tokio::test]
async fn meson_file_trait_fields_are_indexed_on_every_table() {
    let _system = setup_valence().await;
    for table in ["e2e_meson_file", "e2e_meson_profile_photo", OWNED] {
        let schema = valence::SchemaRegistry::global()
            .get_full_schema(table)
            .unwrap_or_else(|| panic!("{table} is registered"));
        for field in [
            "uploaded_by",
            "uploaded_at",
            "file_status",
            "size_bytes",
            "mime_type",
            "file_name",
        ] {
            let f = schema
                .fields
                .iter()
                .find(|f| f.name == field)
                .unwrap_or_else(|| panic!("{table}.{field} missing"));
            assert!(f.indexed || f.unique, "{table}.{field} should be indexed");
        }
    }
}
