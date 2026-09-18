//! Process-wide [`VirusScanner`] install.

use super::{AlwaysCleanScanner, VirusScanner};
use std::sync::{Arc, Mutex, OnceLock};

static INSTALLED: OnceLock<Mutex<Option<Arc<dyn VirusScanner>>>> = OnceLock::new();

fn slot() -> &'static Mutex<Option<Arc<dyn VirusScanner>>> {
    INSTALLED.get_or_init(|| Mutex::new(None))
}

/// Install the process-wide virus scanner used by scan workers.
pub fn install_virus_scanner(scanner: Arc<dyn VirusScanner>) {
    let mut guard = slot()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *guard = Some(scanner);
    tracing::info!(
        target: "meson.virus_scan",
        outcome = "scanner_installed",
        "virus scanner installed"
    );
}

/// Return the installed scanner, if any.
#[must_use]
pub fn installed_virus_scanner() -> Option<Arc<dyn VirusScanner>> {
    let guard = slot()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    guard.clone()
}

/// Resolve the installed scanner, or [`AlwaysCleanScanner`] when none is installed.
#[must_use]
pub fn resolve_virus_scanner() -> Arc<dyn VirusScanner> {
    installed_virus_scanner()
        .unwrap_or_else(|| Arc::new(AlwaysCleanScanner) as Arc<dyn VirusScanner>)
}

/// Clear the process-wide scanner install (tests only).
pub fn clear_virus_scanner_for_test() {
    let mut guard = slot()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *guard = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::AlwaysInfectedScanner;

    #[test]
    fn resolve_defaults_to_always_clean() {
        clear_virus_scanner_for_test();
        assert!(installed_virus_scanner().is_none());
        let _ = resolve_virus_scanner();
        install_virus_scanner(Arc::new(AlwaysInfectedScanner));
        assert!(installed_virus_scanner().is_some());
        clear_virus_scanner_for_test();
    }
}
