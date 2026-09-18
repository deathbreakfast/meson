//! FileQueryAll ownership: list happy + IDOR / preview forbidden sad paths.
//!
//! Scenario IDs: `meson-my-files-list-happy`, `meson-my-files-idor-sad`,
//! `meson-preview-image-happy`, `meson-preview-forbidden-sad`.

mod support;

use meson::generated::{FileFields, FileQueryAll};
use meson::{
    find_file_in_owned_rows, is_uploaded_by, preview_kind, FileByteBackend, LocalDiskBlobStore,
    PreviewKind,
};
use support::{
    as_user, owner_rid, peer_rid, seed_file, seed_profile_photo, setup_valence, OWNER_FILE_ID,
    OWNER_PHOTO_ID, OWNER_USER_ID, PEER_FILE_ID, PEER_USER_ID,
};
use valence::{RecordId, RecordPredicate};

#[tokio::test]
async fn meson_my_files_list_happy() {
    let system = setup_valence().await;
    seed_file(
        &system,
        OWNER_FILE_ID,
        owner_rid(),
        "mine.png",
        "image/png",
        "mine.png",
        12,
    )
    .await;
    seed_file(
        &system,
        PEER_FILE_ID,
        peer_rid(),
        "theirs.pdf",
        "application/pdf",
        "theirs.pdf",
        99,
    )
    .await;

    let owner_v = as_user(&system, OWNER_USER_ID);
    let rows = FileQueryAll::query_used(&owner_v, valence::use_!(r"**Test:** Fixture **File Query All** list for `tests` so the suite can arrange and assert persistence behavior. CI and developers running the suite only."))
        .where_uploaded_by(RecordPredicate::Equals(owner_rid()))
        .await
        .expect("FileQueryAll owner");

    assert_eq!(rows.len(), 1, "owner list must contain only owner uploads");
    assert!(is_uploaded_by(&rows[0], &owner_rid()));
    assert_eq!(rows[0].file_name(), "mine.png");
    let want = RecordId::new("e2e_meson_file", OWNER_FILE_ID);
    assert_eq!(rows[0].id.as_ref(), Some(&want));
}

#[tokio::test]
async fn meson_my_files_idor_sad() {
    let system = setup_valence().await;
    seed_file(
        &system,
        OWNER_FILE_ID,
        owner_rid(),
        "mine.png",
        "image/png",
        "mine.png",
        12,
    )
    .await;

    let peer_v = as_user(&system, PEER_USER_ID);
    let peer_rows = FileQueryAll::query_used(&peer_v, valence::use_!(r"**Test:** Fixture **File Query All** list for `tests` so the suite can arrange and assert persistence behavior. CI and developers running the suite only."))
        .where_uploaded_by(RecordPredicate::Equals(peer_rid()))
        .await
        .expect("FileQueryAll peer");
    assert!(
        peer_rows.is_empty(),
        "peer uploaded_by filter must not return owner rows"
    );

    let owner_want = RecordId::new("e2e_meson_file", OWNER_FILE_ID);
    assert!(
        find_file_in_owned_rows(peer_rows, &owner_want).is_none(),
        "IDOR: foreign file id must not resolve from peer-owned rows"
    );
}

#[tokio::test]
async fn meson_preview_image_happy() {
    let system = setup_valence().await;
    seed_file(
        &system,
        OWNER_FILE_ID,
        owner_rid(),
        "shot.png",
        "image/png",
        "shot.png",
        5,
    )
    .await;

    let root = std::env::temp_dir().join(format!(
        "meson-preview-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let store = LocalDiskBlobStore::new(root.clone());
    store.put("shot.png", b"PNG..").await.expect("put");

    let owner_v = as_user(&system, OWNER_USER_ID);
    let rows = FileQueryAll::query_used(&owner_v, valence::use_!(r"**Test:** Fixture **File Query All** list for `tests` so the suite can arrange and assert persistence behavior. CI and developers running the suite only."))
        .where_uploaded_by(RecordPredicate::Equals(owner_rid()))
        .await
        .expect("list");
    let want = RecordId::new("e2e_meson_file", OWNER_FILE_ID);
    let row = find_file_in_owned_rows(rows, &want).expect("owned row");
    assert_eq!(preview_kind(row.mime_type()), PreviewKind::Image);
    let bytes = store.get(row.storage_path()).await.expect("get");
    assert_eq!(bytes, b"PNG..");

    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn meson_file_query_all_profile_photo_happy() {
    let system = setup_valence().await;
    seed_profile_photo(
        &system,
        OWNER_PHOTO_ID,
        owner_rid(),
        "avatar.png",
        "image/png",
        "avatar.png",
        8,
    )
    .await;

    let owner_v = as_user(&system, OWNER_USER_ID);
    let rows = FileQueryAll::query_used(&owner_v, valence::use_!(r"**Test:** Fixture **File Query All** list for `tests` so the suite can arrange and assert persistence behavior. CI and developers running the suite only."))
        .where_uploaded_by(RecordPredicate::Equals(owner_rid()))
        .await
        .expect("FileQueryAll profile photo");

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].file_name(), "avatar.png");
    let want = RecordId::new("e2e_meson_profile_photo", OWNER_PHOTO_ID);
    assert_eq!(rows[0].id.as_ref(), Some(&want));
}

#[tokio::test]
async fn meson_preview_forbidden_sad() {
    let system = setup_valence().await;
    seed_file(
        &system,
        OWNER_FILE_ID,
        owner_rid(),
        "secret.png",
        "image/png",
        "secret.png",
        5,
    )
    .await;

    let peer_v = as_user(&system, PEER_USER_ID);
    let peer_rows = FileQueryAll::query_used(&peer_v, valence::use_!(r"**Test:** Fixture **File Query All** list for `tests` so the suite can arrange and assert persistence behavior. CI and developers running the suite only."))
        .where_uploaded_by(RecordPredicate::Equals(peer_rid()))
        .await
        .expect("peer list");
    let want = RecordId::new("e2e_meson_file", OWNER_FILE_ID);
    assert!(
        find_file_in_owned_rows(peer_rows, &want).is_none(),
        "peer must not preview owner file via owned-row selection"
    );
}
