//! Codegen output from `schemas/` (`build.rs`).
//!
//! Lint allows cover OUT_DIR-included generated models; lint the schema sources instead.

#![allow(
    missing_docs,
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    clippy::all,
    clippy::pedantic,
    clippy::nursery
)]

use valence::privacy_policies::common::{AUTHENTICATED, SYSTEM_ONLY};

include!(concat!(env!("OUT_DIR"), "/generated_models.rs"));
