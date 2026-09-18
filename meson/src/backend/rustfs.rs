//! RustFS / S3-compatible path-style blob store (`backend-rustfs`).
//!
//! [`RustFsBlobStore`] is [`Clone`]; cloning duplicates credential material into
//! another owned copy — prefer sharing via `Arc` when possible.

use super::keys::validate_object_key;
use super::sigv4::sign_s3_request;
use super::{FileByteBackend, FileStoreError};
use async_trait::async_trait;
use reqwest::StatusCode;
use std::fmt;

/// Default SigV4 region when `MESON_RUSTFS_REGION` is unset.
pub const DEFAULT_RUSTFS_REGION: &str = "us-east-1";

/// S3 path-style blob store targeting RustFS (or any compatible endpoint).
///
/// Credentials are redacted in [`Debug`] output.
#[derive(Clone)]
pub struct RustFsBlobStore {
    endpoint: String,
    bucket: String,
    access_key: String,
    secret_key: String,
    region: String,
    client: reqwest::Client,
    /// When set, put/get reject objects larger than this many bytes.
    max_object_bytes: Option<usize>,
}

impl fmt::Debug for RustFsBlobStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RustFsBlobStore")
            .field("endpoint", &self.endpoint)
            .field("bucket", &self.bucket)
            .field("access_key", &"***")
            .field("secret_key", &"***")
            .field("region", &self.region)
            .field("max_object_bytes", &self.max_object_bytes)
            .finish_non_exhaustive()
    }
}

impl RustFsBlobStore {
    /// Build a store for `endpoint` + `bucket` with static credentials.
    ///
    /// # Errors
    ///
    /// Returns [`FileStoreError::Io`] when the HTTP client cannot be built.
    pub fn new(
        endpoint: impl Into<String>,
        bucket: impl Into<String>,
        access_key: impl Into<String>,
        secret_key: impl Into<String>,
        region: impl Into<String>,
    ) -> Result<Self, FileStoreError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(FileStoreError::io)?;
        Ok(Self {
            endpoint: endpoint.into().trim_end_matches('/').to_string(),
            bucket: bucket.into().trim_matches('/').to_string(),
            access_key: access_key.into(),
            secret_key: secret_key.into(),
            region: region.into(),
            client,
            max_object_bytes: None,
        })
    }

    /// Cap put/get body size (bytes). `None` leaves the previous limit unchanged
    /// only when chaining; pass `Some`/`None` explicitly via this setter.
    #[must_use]
    pub fn with_max_object_bytes(mut self, max_object_bytes: Option<usize>) -> Self {
        self.max_object_bytes = max_object_bytes;
        self
    }

    fn enforce_max_object_bytes(&self, len: usize) -> Result<(), FileStoreError> {
        if let Some(max) = self.max_object_bytes {
            if len > max {
                tracing::warn!(
                    backend = "rustfs",
                    outcome = "object_too_large",
                    len,
                    max,
                    "object exceeds max_object_bytes"
                );
                return Err(FileStoreError::io_msg(format!(
                    "object exceeds max_object_bytes ({max})"
                )));
            }
        }
        Ok(())
    }

    fn object_url(&self, key: &str) -> String {
        format!("{}/{}/{}", self.endpoint, self.bucket, key)
    }

    fn bucket_url(&self) -> String {
        format!("{}/{}", self.endpoint, self.bucket)
    }

    fn map_http_failure(status: StatusCode) -> FileStoreError {
        if status == StatusCode::FORBIDDEN || status == StatusCode::UNAUTHORIZED {
            FileStoreError::Unauthorized
        } else if status == StatusCode::NOT_FOUND {
            FileStoreError::NotFound
        } else {
            FileStoreError::io_http(status.as_u16())
        }
    }

    /// Ensure the configured bucket exists (CreateBucket; 409 is ok).
    ///
    /// # Errors
    ///
    /// Auth failures → [`FileStoreError::Unauthorized`]; other HTTP → [`FileStoreError::Io`].
    pub async fn ensure_bucket(&self) -> Result<(), FileStoreError> {
        let url = self.bucket_url();
        let (authorization, amz_date, payload_hash) = sign_s3_request(
            "PUT",
            &url,
            b"",
            &self.access_key,
            &self.secret_key,
            &self.region,
            &[("content-type", "application/octet-stream")],
        );
        let res = self
            .client
            .put(&url)
            .header("Authorization", authorization)
            .header("x-amz-date", amz_date)
            .header("x-amz-content-sha256", payload_hash)
            .header("Content-Type", "application/octet-stream")
            .body(Vec::<u8>::new())
            .send()
            .await
            .map_err(|e| {
                tracing::warn!(backend = "rustfs", outcome = "ensure_bucket_transport", error = %e);
                FileStoreError::io(e)
            })?;
        let status = res.status();
        if status.is_success() || status.as_u16() == 409 {
            Ok(())
        } else if status == StatusCode::FORBIDDEN || status == StatusCode::UNAUTHORIZED {
            tracing::warn!(
                backend = "rustfs",
                outcome = "ensure_bucket_unauthorized",
                status = status.as_u16()
            );
            Err(FileStoreError::Unauthorized)
        } else {
            tracing::warn!(
                backend = "rustfs",
                outcome = "ensure_bucket_http",
                status = status.as_u16()
            );
            Err(FileStoreError::io_http(status.as_u16()))
        }
    }

    async fn signed_put(&self, url: &str, body: &[u8]) -> Result<StatusCode, FileStoreError> {
        let (authorization, amz_date, payload_hash) = sign_s3_request(
            "PUT",
            url,
            body,
            &self.access_key,
            &self.secret_key,
            &self.region,
            &[("content-type", "application/octet-stream")],
        );
        let res = self
            .client
            .put(url)
            .header("Authorization", authorization)
            .header("x-amz-date", amz_date)
            .header("x-amz-content-sha256", payload_hash)
            .header("Content-Type", "application/octet-stream")
            .body(body.to_vec())
            .send()
            .await
            .map_err(|e| {
                tracing::warn!(backend = "rustfs", outcome = "put_transport", error = %e);
                FileStoreError::io(e)
            })?;
        Ok(res.status())
    }

    async fn signed_get(&self, url: &str) -> Result<(StatusCode, Vec<u8>), FileStoreError> {
        let (authorization, amz_date, payload_hash) = sign_s3_request(
            "GET",
            url,
            b"",
            &self.access_key,
            &self.secret_key,
            &self.region,
            &[],
        );
        let res = self
            .client
            .get(url)
            .header("Authorization", authorization)
            .header("x-amz-date", amz_date)
            .header("x-amz-content-sha256", payload_hash)
            .send()
            .await
            .map_err(|e| {
                tracing::warn!(backend = "rustfs", outcome = "get_transport", error = %e);
                FileStoreError::io(e)
            })?;
        let status = res.status();
        let bytes = res.bytes().await.map_err(|e| {
            tracing::warn!(backend = "rustfs", outcome = "get_body", error = %e);
            FileStoreError::io(e)
        })?;
        Ok((status, bytes.to_vec()))
    }

    async fn signed_delete(&self, url: &str) -> Result<StatusCode, FileStoreError> {
        let (authorization, amz_date, payload_hash) = sign_s3_request(
            "DELETE",
            url,
            b"",
            &self.access_key,
            &self.secret_key,
            &self.region,
            &[],
        );
        let res = self
            .client
            .delete(url)
            .header("Authorization", authorization)
            .header("x-amz-date", amz_date)
            .header("x-amz-content-sha256", payload_hash)
            .send()
            .await
            .map_err(|e| {
                tracing::warn!(backend = "rustfs", outcome = "delete_transport", error = %e);
                FileStoreError::io(e)
            })?;
        Ok(res.status())
    }
}

#[async_trait]
impl FileByteBackend for RustFsBlobStore {
    async fn put(&self, key: &str, bytes: &[u8]) -> Result<(), FileStoreError> {
        validate_object_key(key)?;
        self.enforce_max_object_bytes(bytes.len())?;
        let url = self.object_url(key);
        let status = self.signed_put(&url, bytes).await?;
        if status.is_success() {
            Ok(())
        } else if status == StatusCode::FORBIDDEN || status == StatusCode::UNAUTHORIZED {
            tracing::warn!(
                backend = "rustfs",
                outcome = "put_unauthorized",
                status = status.as_u16()
            );
            Err(FileStoreError::Unauthorized)
        } else {
            tracing::warn!(
                backend = "rustfs",
                outcome = "put_http",
                status = status.as_u16()
            );
            Err(Self::map_http_failure(status))
        }
    }

    async fn get(&self, key: &str) -> Result<Vec<u8>, FileStoreError> {
        validate_object_key(key)?;
        let url = self.object_url(key);
        let (status, bytes) = self.signed_get(&url).await?;
        if status.is_success() {
            self.enforce_max_object_bytes(bytes.len())?;
            Ok(bytes)
        } else if status == StatusCode::NOT_FOUND {
            Err(FileStoreError::NotFound)
        } else if status == StatusCode::FORBIDDEN || status == StatusCode::UNAUTHORIZED {
            tracing::warn!(
                backend = "rustfs",
                outcome = "get_unauthorized",
                status = status.as_u16()
            );
            Err(FileStoreError::Unauthorized)
        } else {
            tracing::warn!(
                backend = "rustfs",
                outcome = "get_http",
                status = status.as_u16()
            );
            Err(Self::map_http_failure(status))
        }
    }

    async fn delete(&self, key: &str) -> Result<(), FileStoreError> {
        validate_object_key(key)?;
        let url = self.object_url(key);
        let status = self.signed_delete(&url).await?;
        if status.is_success() || status == StatusCode::NOT_FOUND {
            Ok(())
        } else if status == StatusCode::FORBIDDEN || status == StatusCode::UNAUTHORIZED {
            tracing::warn!(
                backend = "rustfs",
                outcome = "delete_unauthorized",
                status = status.as_u16()
            );
            Err(FileStoreError::Unauthorized)
        } else {
            tracing::warn!(
                backend = "rustfs",
                outcome = "delete_http",
                status = status.as_u16()
            );
            Err(Self::map_http_failure(status))
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    #[test]
    fn debug_redacts_credentials() {
        let store = RustFsBlobStore::new(
            "http://127.0.0.1:9000",
            "bucket",
            "AKIASECRETACCESS",
            "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
            "us-east-1",
        )
        .expect("client");
        let dbg = format!("{store:?}");
        assert!(dbg.contains("***"));
        assert!(!dbg.contains("AKIASECRETACCESS"));
        assert!(!dbg.contains("wJalrXUtnFEMI"));
    }

    #[tokio::test]
    async fn max_object_bytes_rejects_put_without_network() {
        let store = RustFsBlobStore::new("http://127.0.0.1:9", "bucket", "k", "s", "us-east-1")
            .expect("client")
            .with_max_object_bytes(Some(4));
        let err = store.put("ok.bin", b"12345").await.expect_err("oversize");
        assert!(matches!(err, FileStoreError::Io { .. }));
        assert_eq!(err.to_string(), "storage I/O error");
    }
}
