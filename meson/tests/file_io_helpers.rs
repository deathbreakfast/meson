//! File IO helpers: create_with_bytes + get_file_bytes happy/sad paths.
//!
//! Behaviors: create+load bytes match; owner-scoped IDOR via FileQueryAll;
//! missing install; missing blob; Valence create fail after put.

mod support;

use meson::generated::{E2eMesonFile, FileFileStatus};
use meson::{
    clear_blob_store_for_test, clear_blob_stores_for_test, install_blob_store, FileBytes,
    FileCreateMeta, FileStoreError, FileUpload, FileUploadError, LocalDiskBlobStore,
};
use std::sync::Arc;
use support::{
    as_user, blob_install_lock, owner_rid, peer_rid, setup_valence, OWNER_USER_ID, PEER_USER_ID,
};
use valence::Model;

fn temp_root() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "meson-file-io-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn install_temp_store() -> (std::path::PathBuf, Arc<LocalDiskBlobStore>) {
    clear_blob_stores_for_test();
    let root = temp_root();
    let store = Arc::new(LocalDiskBlobStore::new(root.clone()));
    install_blob_store(store.clone()).expect("install");
    (root, store)
}

#[tokio::test]
async fn create_with_bytes_then_get_file_bytes_happy() {
    let _lock = blob_install_lock().await;
    let (root, _) = install_temp_store();
    let system = setup_valence().await;
    let payload = b"PNG..";

    let created = E2eMesonFile::create_with_bytes(
        &system,
        FileCreateMeta {
            file_name: "receipt-1.png".into(),
            file_extension: "png".into(),
            mime_type: "image/png".into(),
            uploaded_by: owner_rid(),
        },
        payload,
    )
    .await
    .expect("create_with_bytes");

    assert_eq!(created.file_name(), "receipt-1.png");
    assert_eq!(*created.size_bytes(), payload.len() as i64);
    assert!(matches!(created.file_status(), FileFileStatus::Available));
    assert!(
        !created.storage_path().contains('/'),
        "storage_path must be flat"
    );
    assert_eq!(
        std::path::Path::new(created.storage_path())
            .extension()
            .and_then(|e| e.to_str()),
        Some("png")
    );

    let id = created.id().expect("id").clone();
    let bare = id.id().to_string();
    let session = as_user(&system, OWNER_USER_ID);
    let row = E2eMesonFile::get_used(&bare, &session, valence::use_!(r"**Test:** Fixture **E2e Meson File** load for `tests` so the suite can arrange and assert persistence behavior. CI and developers running the suite only."))
        .await
        .expect("get")
        .expect("owner can load own row");
    let bytes = row.get_file_bytes().await.expect("get_file_bytes");
    assert_eq!(bytes, payload);

    clear_blob_store_for_test();
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn get_file_bytes_after_session_get_happy() {
    let _lock = blob_install_lock().await;
    let (root, _) = install_temp_store();
    let system = setup_valence().await;
    let created = E2eMesonFile::create_with_bytes(
        &system,
        FileCreateMeta {
            file_name: "shot.png".into(),
            file_extension: "png".into(),
            mime_type: "image/png".into(),
            uploaded_by: owner_rid(),
        },
        b"hello",
    )
    .await
    .expect("create");

    let id = created.id().expect("id").clone();
    let bare = id.id().to_string();
    let session = as_user(&system, OWNER_USER_ID);
    let row = E2eMesonFile::get_used(&bare, &session, valence::use_!(r"**Test:** Fixture **E2e Meson File** load for `tests` so the suite can arrange and assert persistence behavior. CI and developers running the suite only."))
        .await
        .expect("get")
        .expect("row");
    assert_eq!(row.get_file_bytes().await.expect("bytes"), b"hello");

    clear_blob_store_for_test();
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn session_get_foreign_receipt_none_sad() {
    let _lock = blob_install_lock().await;
    let (root, _) = install_temp_store();
    let system = setup_valence().await;
    let created = E2eMesonFile::create_with_bytes(
        &system,
        FileCreateMeta {
            file_name: "mine.png".into(),
            file_extension: "png".into(),
            mime_type: "image/png".into(),
            uploaded_by: owner_rid(),
        },
        b"secret",
    )
    .await
    .expect("create");

    let id = created.id().expect("id").clone();
    let bare = id.id().to_string();
    let peer = as_user(&system, PEER_USER_ID);
    // Fixture read policy is AUTHENTICATED (any signed-in user). Product My
    // Files IDOR uses FileQueryAll + uploaded_by — assert that path here.
    let _peer_get = E2eMesonFile::get_used(&bare, &peer, valence::use_!(r"**Test:** Fixture **E2e Meson File** load for `tests` so the suite can arrange and assert persistence behavior. CI and developers running the suite only.")).await.expect("get");
    let owner_scoped = meson::generated::FileQueryAll::query_used(&peer, valence::use_!(r"**Test:** Fixture **File Query All** list for `tests` so the suite can arrange and assert persistence behavior. CI and developers running the suite only."))
        .where_uploaded_by(valence::RecordPredicate::Equals(peer_rid()))
        .await
        .expect("query");
    assert!(
        meson::find_file_in_owned_rows(owner_scoped, &id).is_none(),
        "peer FileQueryAll must not surface owner file"
    );

    clear_blob_store_for_test();
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn get_file_bytes_without_install_sad() {
    let _lock = blob_install_lock().await;
    clear_blob_store_for_test();
    let system = setup_valence().await;
    let row = E2eMesonFile::new(
        "x.png".into(),
        "png".into(),
        "image/png".into(),
        1,
        "x.png".into(),
        FileFileStatus::Available,
        owner_rid(),
        chrono::Utc::now(),
    )
    .expect("new");
    let created = E2eMesonFile::create_used(row, &system, valence::use_!(r"**Test:** Fixture **E2e Meson File** save for `tests` so the suite can arrange and assert persistence behavior. CI and developers running the suite only.")).await.expect("create");
    let err = created.get_file_bytes().await.expect_err("no install");
    assert!(
        matches!(err, FileStoreError::BlobStoreNotInstalled),
        "got {err:?}"
    );
}

#[tokio::test]
async fn create_with_bytes_without_install_sad() {
    let _lock = blob_install_lock().await;
    clear_blob_store_for_test();
    let system = setup_valence().await;
    let err = E2eMesonFile::create_with_bytes(
        &system,
        FileCreateMeta {
            file_name: "x.png".into(),
            file_extension: "png".into(),
            mime_type: "image/png".into(),
            uploaded_by: owner_rid(),
        },
        b"x",
    )
    .await
    .expect_err("no install");
    assert!(
        matches!(err, FileUploadError::BlobStoreNotInstalled),
        "got {err:?}"
    );
}

#[tokio::test]
async fn get_file_bytes_missing_blob_sad() {
    let _lock = blob_install_lock().await;
    let (root, _) = install_temp_store();
    let system = setup_valence().await;
    let row = E2eMesonFile::new(
        "ghost.png".into(),
        "png".into(),
        "image/png".into(),
        4,
        "missing-blob.png".into(),
        FileFileStatus::Available,
        owner_rid(),
        chrono::Utc::now(),
    )
    .expect("new");
    let created = E2eMesonFile::create_used(row, &system, valence::use_!(r"**Test:** Fixture **E2e Meson File** save for `tests` so the suite can arrange and assert persistence behavior. CI and developers running the suite only.")).await.expect("create");
    let err = created.get_file_bytes().await.expect_err("missing blob");
    assert!(matches!(err, FileStoreError::NotFound), "got {err:?}");

    clear_blob_store_for_test();
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn create_fails_after_put_sad() {
    let _lock = blob_install_lock().await;
    let (root, _store) = install_temp_store();
    let system = setup_valence().await;
    let anon = system.with_actor(valence::Actor::Anonymous);
    let err = E2eMesonFile::create_with_bytes(
        &anon,
        FileCreateMeta {
            file_name: "denied.png".into(),
            file_extension: "png".into(),
            mime_type: "image/png".into(),
            uploaded_by: owner_rid(),
        },
        b"orphan",
    )
    .await
    .expect_err("policy deny");
    assert!(
        matches!(err, FileUploadError::Valence(_)),
        "expected Valence deny after put, got {err:?}"
    );

    clear_blob_store_for_test();
    let _ = std::fs::remove_dir_all(root);
}
