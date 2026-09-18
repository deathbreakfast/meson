//! Virus scanner install and implementations.

mod always_clean;
#[cfg(feature = "scanner-clamav")]
mod clamav;
mod install;

pub use always_clean::AlwaysCleanScanner;
#[cfg(feature = "scanner-clamav")]
pub use clamav::{parse_clamd_response, ClamAvConfigError, ClamAvScanner, ClamdResponse};
pub use install::{
    clear_virus_scanner_for_test, install_virus_scanner, installed_virus_scanner,
    resolve_virus_scanner,
};

use async_trait::async_trait;
use thiserror::Error;

/// Result of scanning opaque file bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanVerdict {
    /// No infection detected.
    Clean,
    /// Infection detected. Display stays free of AV signature strings.
    Infected {
        /// Opaque class for metrics (for example `malware`); never a signature name.
        reason_class: Option<&'static str>,
    },
}

impl std::fmt::Display for ScanVerdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Clean => f.write_str("clean"),
            Self::Infected { reason_class } => match reason_class {
                Some(c) => write!(f, "infected:{c}"),
                None => f.write_str("infected"),
            },
        }
    }
}

/// Errors from [`VirusScanner::scan`].
#[derive(Debug, Error)]
pub enum ScanError {
    /// Scanner transport / protocol failure (retryable in some cases).
    #[error("scanner error")]
    Scanner {
        /// Opaque reason class for logs/metrics (no payload bytes).
        reason_class: Option<&'static str>,
        /// Underlying cause when known.
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },
}

impl ScanError {
    /// Build a scanner error with an optional reason class and typed source.
    #[must_use]
    pub fn scanner(
        reason_class: Option<&'static str>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::Scanner {
            reason_class,
            source: Some(Box::new(source)),
        }
    }

    /// Build a scanner error with only a reason class.
    #[must_use]
    pub fn scanner_msg(reason_class: &'static str) -> Self {
        Self::Scanner {
            reason_class: Some(reason_class),
            source: None,
        }
    }
}

/// Async virus scanner over opaque bytes.
#[async_trait]
pub trait VirusScanner: Send + Sync {
    /// Scan `bytes` and return a verdict.
    ///
    /// # Errors
    ///
    /// Returns [`ScanError`] when the scanner cannot produce a verdict.
    async fn scan(&self, bytes: &[u8]) -> Result<ScanVerdict, ScanError>;
}

/// Test helper that always reports infection (no signature strings).
#[derive(Debug, Default, Clone, Copy)]
pub struct AlwaysInfectedScanner;

#[async_trait]
impl VirusScanner for AlwaysInfectedScanner {
    async fn scan(&self, _bytes: &[u8]) -> Result<ScanVerdict, ScanError> {
        Ok(ScanVerdict::Infected {
            reason_class: Some("test"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn always_clean_and_infected() {
        let clean = AlwaysCleanScanner;
        assert_eq!(clean.scan(b"x").await.unwrap(), ScanVerdict::Clean);
        let dirty = AlwaysInfectedScanner;
        assert!(matches!(
            dirty.scan(b"x").await.unwrap(),
            ScanVerdict::Infected { .. }
        ));
        assert_eq!(ScanVerdict::Clean.to_string(), "clean");
        assert_eq!(
            ScanVerdict::Infected {
                reason_class: Some("test")
            }
            .to_string(),
            "infected:test"
        );
        assert!(!ScanVerdict::Infected { reason_class: None }
            .to_string()
            .contains("EICAR"));
    }
}
