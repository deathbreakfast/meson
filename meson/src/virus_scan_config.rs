//! `MESON_VIRUS_SCAN` env gate for quarantine-first upload.

/// Whether virus-scan quarantine upload is enabled.
///
/// Returns `false` only when `MESON_VIRUS_SCAN` is set to `off` (case
/// insensitive). Missing or any other value keeps scan **on**.
#[must_use]
pub fn virus_scan_enabled() -> bool {
    !matches!(
        std::env::var("MESON_VIRUS_SCAN"),
        Ok(v) if v.eq_ignore_ascii_case("off")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enabled_by_default_and_off() {
        std::env::remove_var("MESON_VIRUS_SCAN");
        assert!(virus_scan_enabled());
        std::env::set_var("MESON_VIRUS_SCAN", "off");
        assert!(!virus_scan_enabled());
        std::env::set_var("MESON_VIRUS_SCAN", "OFF");
        assert!(!virus_scan_enabled());
        std::env::set_var("MESON_VIRUS_SCAN", "on");
        assert!(virus_scan_enabled());
        std::env::remove_var("MESON_VIRUS_SCAN");
    }
}
