//! Zips over the inline limit are built by the `meson_zip_files` Boson task, as the user.
//!
//! Scenario ID: `meson-zip-background`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

mod support;

use std::sync::Arc;

use boson_backend_mem::MemQueueBackend;
use boson_core::JobStatus;
use boson_runtime::{configure, Boson};
use boson_valence_identity::{
    router_config_reject_external_system, ValenceExecutionContextFactory,
};
use meson::generated::FileFileStatus;
use meson::{
    clear_blob_stores_for_test, install_blob_store, install_quarantine_store, zip_files,
    zip_object_key, FileByteBackend, LocalDiskBlobStore, SkipReason, ZipOutput,
};
use support::{
    as_user, blob_install_lock, owner_rid, seed_owned_file, setup_valence_with_router,
    OWNER_USER_ID,
};
use valence::{ActorTrust, RouterValenceFactory};

fn temp_root(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "meson-zipbg-{label}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[tokio::test]
async fn meson_zip_over_one_gb_is_built_in_the_background() {
    let _g = blob_install_lock().await;
    clear_blob_stores_for_test();
    let available = Arc::new(LocalDiskBlobStore::new(temp_root("a")));
    install_blob_store(available.clone()).unwrap();
    install_quarantine_store(Arc::new(LocalDiskBlobStore::new(temp_root("q")))).unwrap();

    let (system, router, default_key) = setup_valence_with_router().await;
    // Force-link the task into Boson's inventory for this test binary.
    let _ = std::any::type_name::<meson::jobs::MesonZipFilesParams>();
    let mut cfg = router_config_reject_external_system(default_key);
    cfg.actor_trust = ActorTrust::Internal;
    let factory = RouterValenceFactory::arc(router, cfg);
    let (boson, worker) = Boson::builder()
        .queue_backend(Arc::new(MemQueueBackend::new()))
        .execution_context_factory(ValenceExecutionContextFactory::new(factory))
        .auto_registry()
        .without_worker()
        .build_manual()
        .expect("manual boson");
    configure(boson.clone());

    // Recorded size over 1 GB; the stored bytes are small.
    available.put("big.bin", b"big bytes").await.unwrap();
    let big = seed_owned_file(
        &system,
        "big",
        owner_rid(),
        "big.iso",
        "big.bin",
        1_500_000_000,
        FileFileStatus::Available,
    )
    .await;
    let quarantined = seed_owned_file(
        &system,
        "q",
        owner_rid(),
        "q.png",
        "q.bin",
        1,
        FileFileStatus::Quarantined,
    )
    .await;
    let owner_v = as_user(&system, OWNER_USER_ID);

    let result = zip_files(&owner_v, &owner_rid(), &[big.clone(), quarantined.clone()])
        .await
        .unwrap();
    let ZipOutput::Queued { task_id } = result.output else {
        panic!("expected a background zip");
    };
    assert_eq!(result.included, vec![big]);
    assert_eq!(result.skipped, vec![(quarantined, SkipReason::Quarantined)]);

    while worker.try_run_next().await {}
    assert_eq!(boson.count_jobs(Some(JobStatus::Success)).await.unwrap(), 1);

    let bytes = available.get(&zip_object_key(&task_id)).await.unwrap();
    let archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let names: Vec<String> = archive
        .file_names()
        .map(|n| n.unwrap().into_owned())
        .collect();
    assert_eq!(names, vec!["big.iso".to_string()]);
}
