//! Scanner that always returns [`ScanVerdict::Clean`].

use super::{ScanError, ScanVerdict, VirusScanner};
use async_trait::async_trait;

/// Default / embedded scanner: treats every payload as clean.
#[derive(Debug, Default, Clone, Copy)]
pub struct AlwaysCleanScanner;

#[async_trait]
impl VirusScanner for AlwaysCleanScanner {
    async fn scan(&self, _bytes: &[u8]) -> Result<ScanVerdict, ScanError> {
        Ok(ScanVerdict::Clean)
    }
}
