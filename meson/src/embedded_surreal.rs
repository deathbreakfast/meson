//! Logical embedded database name for Meson Valence schemas.

use valence::{Database, DatabaseFromEngine, SQLITE_ENGINE_ID};

/// Logical database name Meson schemas are registered under.
pub const DEFAULT_LOGICAL_NAME: &str = "default";

const ENGINE_ID: &str = SQLITE_ENGINE_ID;

/// [`DatabaseFromEngine`] pointing at [`DEFAULT_LOGICAL_NAME`] on embedded SQLite.
pub const DEFAULT_STORAGE: DatabaseFromEngine =
    Database::from_engine(DEFAULT_LOGICAL_NAME, ENGINE_ID);

/// Logical names for test / host routers that link Meson File models.
pub const EMBEDDED_SURREAL_LOGICAL_NAMES: &[&str] = &[DEFAULT_LOGICAL_NAME];
