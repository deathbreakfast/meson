//! Virus-scan quarantine: dual store, pending create, status gate, promote.

mod support;

use meson::generated::{E2eMesonFile, FileFileStatus};
use meson::{
    clear_blob_stores_for_test, get_available_file_bytes, install_blob_store,
    install_quarantine_store, install_virus_scanner, promote_to_available, AlwaysCleanScanner,
    AlwaysInfectedScanner, FileBytes, FileCreateMeta, FileStoreError, FileUpload,
    LocalDiskBlobStore, ScanVerdict, VirusScanner,
};
use std::sync::Arc;
use support::{as_user, blob_install_lock, owner_rid, setup_valence, OWNER_USER_ID};
use valence::Model;

fn temp_root(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "meson-vs-{label}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

async fn install_dual() {
    clear_blob_stores_for_test();
    let avail = temp_root("a");
    let quar = temp_root("q");
    install_blob_store(Arc::new(LocalDiskBlobStore::new(&avail))).unwrap();
    install_quarantine_store(Arc::new(LocalDiskBlobStore::new(&quar))).unwrap();
    install_virus_scanner(Arc::new(AlwaysCleanScanner));
}

#[tokio::test]
async fn quarantine_upload_pending_then_promote_happy() {
    let _lock = blob_install_lock().await;
    // SAFETY: under blob_install_lock
    unsafe {
        std::env::set_var("MESON_VIRUS_SCAN", "on");
    }
    install_dual().await;
    let system = setup_valence().await;
    let payload = b"PNG-clean";

    let created = E2eMesonFile::create_with_bytes(
        &system,
        FileCreateMeta {
            file_name: "x.png".into(),
            file_extension: "png".into(),
            mime_type: "image/png".into(),
            uploaded_by: owner_rid(),
        },
        payload,
    )
    .await
    .expect("create");

    assert!(matches!(
        created.file_status(),
        FileFileStatus::PendingVirusScan
    ));
    let session = as_user(&system, OWNER_USER_ID);
    let bare = created.id().unwrap().id().to_string();
    let row = E2eMesonFile::get_used(&bare, &session, valence::use_!(r"**Test:** Fixture **E2e Meson File** load for `tests` so the suite can arrange and assert persistence behavior. CI and developers running the suite only.")).await.unwrap().unwrap();
    assert!(matches!(
        row.get_file_bytes().await,
        Err(FileStoreError::NotAvailable { .. })
    ));

    let path = promote_to_available(row.storage_path()).await.unwrap();
    row.get_mutable_used(&system, valence::use_!(r"**Test:** Fixture **E2e Meson File Mutable** update for `tests` so the suite can arrange and assert persistence behavior. CI and developers running the suite only."))
        .set_storage_path(path)
        .unwrap()
        .set_file_status(FileFileStatus::Available)
        .unwrap()
        .commit()
        .await
        .unwrap();

    let fresh = E2eMesonFile::get_used(&bare, &session, valence::use_!(r"**Test:** Fixture **E2e Meson File** load for `tests` so the suite can arrange and assert persistence behavior. CI and developers running the suite only.")).await.unwrap().unwrap();
    let bytes = fresh.get_file_bytes().await.unwrap();
    assert_eq!(bytes, payload);
    let via = get_available_file_bytes(&system, "e2e_meson_file", &bare)
        .await
        .unwrap();
    assert_eq!(via, payload);

    unsafe {
        std::env::set_var("MESON_VIRUS_SCAN", "off");
    }
    clear_blob_stores_for_test();
}

#[tokio::test]
async fn infected_scanner_verdict() {
    let v = AlwaysInfectedScanner.scan(b"x").await.unwrap();
    assert!(matches!(v, ScanVerdict::Infected { .. }));
}

#[tokio::test]
async fn opt_out_available_immediate() {
    let _lock = blob_install_lock().await;
    unsafe {
        std::env::set_var("MESON_VIRUS_SCAN", "off");
    }
    install_dual().await;
    let system = setup_valence().await;
    let created = E2eMesonFile::create_with_bytes(
        &system,
        FileCreateMeta {
            file_name: "y.png".into(),
            file_extension: "png".into(),
            mime_type: "image/png".into(),
            uploaded_by: owner_rid(),
        },
        b"ok",
    )
    .await
    .unwrap();
    assert!(matches!(created.file_status(), FileFileStatus::Available));
    clear_blob_stores_for_test();
}
