//! End-to-end File upload + load with [`meson::ReceiptScan`].
//!
//! ## What this runs
//!
//! 1. Install a temp [`LocalDiskBlobStore`] via [`meson::install_blob_store`].
//! 2. Boot in-memory SQLite Valence and sync typed tables.
//! 3. Create a receipt under System with [`FileUpload::create_with_bytes`].
//! 4. Session-load with [`Model::get`] + [`FileBytes::get_file_bytes`].
//!
//! ## Command
//!
//! ```bash
//! CARGO_BUILD_JOBS=1 cargo run -p meson --example upload_and_load
//! ```
//!
//! ## Success
//!
//! Stdout prints `ok uploaded+loaded 5 bytes`.

#![allow(clippy::print_stdout, clippy::expect_used, missing_docs)]

use meson::{
    clear_blob_store_for_test, install_blob_store, touch_schema_inventory, FileBytes,
    FileCreateMeta, FileUpload, LocalDiskBlobStore, ReceiptScan,
};
use std::sync::Arc;
use valence::{
    register_backend_logical_names, Actor, DatabaseBackend, DatabaseRouter, Model, RecordId,
    RegisterBackendLogicalNamesOptions, SqliteBackend, Valence, SQLITE_ENGINE_ID,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    clear_blob_store_for_test();
    let root = std::env::temp_dir().join(format!(
        "meson-upload-and-load-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    install_blob_store(Arc::new(LocalDiskBlobStore::new(root.clone())))?;

    valence::deletion::register_noop_deletion_dispatcher_for_tests();
    valence::clear_for_test();
    touch_schema_inventory();

    if std::env::var_os("VALENCE_OWNERSHIP_UNIFIED_FETCH").is_none() {
        // SAFETY: example harness; matches Meson integration tests.
        unsafe {
            std::env::set_var("VALENCE_OWNERSHIP_UNIFIED_FETCH", "0");
        }
    }

    let backend: Arc<dyn DatabaseBackend> =
        Arc::new(SqliteBackend::connect_memory().await.expect("sqlite mem"));
    let mut router = DatabaseRouter::new();
    register_backend_logical_names(
        &mut router,
        backend,
        &["default"],
        RegisterBackendLogicalNamesOptions::default(),
    );
    let system_v = Valence::builder()
        .database_router(Arc::new(router))
        .default_backend_key(valence::router_key("default", SQLITE_ENGINE_ID))
        .with_actor(Actor::System {
            operation: "upload_and_load_example".into(),
        })
        .build()?;
    system_v.sync_typed_tables_from_registry().await?;

    let owner = RecordId::new("user", "alice");
    let created = ReceiptScan::create_with_bytes(
        &system_v,
        FileCreateMeta {
            file_name: "receipt-1.png".into(),
            file_extension: "png".into(),
            mime_type: "image/png".into(),
            uploaded_by: owner,
        },
        b"PNG..",
    )
    .await?;

    let bare = created.id().expect("id").id().to_string();
    let session_v = system_v.with_actor(Actor::User {
        user_id: "alice".into(),
    });
    let row = ReceiptScan::get_used(&bare, &session_v, valence::use_!(r"In **Meson file storage**, we **load Receipt Scan** so the application can decide what to do next in this workflow. The result is used by **Meson file storage** logic—not necessarily displayed on a page unless that feature’s UI shows it."))
        .await?
        .ok_or("not found")?;
    let loaded = row.get_file_bytes().await?;
    assert_eq!(loaded, b"PNG..");
    println!("ok uploaded+loaded {} bytes", loaded.len());

    clear_blob_store_for_test();
    let _ = std::fs::remove_dir_all(root);
    Ok(())
}
