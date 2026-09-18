//! ClamAV `clamd` client via Unix socket or TCP (INSTREAM protocol).
//!
//! Does **not** bundle a ClamAV binary. Hosts run `clamd` and point Meson at it
//! with `MESON_CLAMD_SOCKET` or `MESON_CLAMD_HOST` / `MESON_CLAMD_PORT`.

use super::{ScanError, ScanVerdict, VirusScanner};
use async_trait::async_trait;
use std::path::PathBuf;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpStream, UnixStream};

/// Errors building [`ClamAvScanner`] from env.
#[derive(Debug, Error)]
pub enum ClamAvConfigError {
    /// Neither Unix socket nor TCP host/port configured.
    #[error("missing clamd endpoint (set MESON_CLAMD_SOCKET or MESON_CLAMD_HOST)")]
    MissingEndpoint,
    /// `MESON_CLAMD_PORT` present but not parseable.
    #[error("invalid MESON_CLAMD_PORT")]
    InvalidPort,
}

/// Where to reach `clamd`.
#[derive(Debug, Clone)]
pub enum ClamdEndpoint {
    /// Unix domain socket path.
    Unix(PathBuf),
    /// TCP host + port.
    Tcp { host: String, port: u16 },
}

/// ClamAV scanner talking to a running `clamd`.
#[derive(Debug, Clone)]
pub struct ClamAvScanner {
    endpoint: ClamdEndpoint,
}

impl ClamAvScanner {
    /// Build a scanner for `endpoint`.
    #[must_use]
    pub fn new(endpoint: ClamdEndpoint) -> Self {
        Self { endpoint }
    }

    /// Build from `MESON_CLAMD_SOCKET` or `MESON_CLAMD_HOST` (+ optional port).
    ///
    /// Default TCP port is `3310` when host is set without port.
    ///
    /// # Errors
    ///
    /// [`ClamAvConfigError`] when neither socket nor host is set, or port is invalid.
    pub fn from_env() -> Result<Self, ClamAvConfigError> {
        if let Ok(sock) = std::env::var("MESON_CLAMD_SOCKET") {
            if !sock.is_empty() {
                return Ok(Self::new(ClamdEndpoint::Unix(PathBuf::from(sock))));
            }
        }
        let host =
            std::env::var("MESON_CLAMD_HOST").map_err(|_| ClamAvConfigError::MissingEndpoint)?;
        if host.is_empty() {
            return Err(ClamAvConfigError::MissingEndpoint);
        }
        let port = match std::env::var("MESON_CLAMD_PORT") {
            Ok(p) if !p.is_empty() => p.parse().map_err(|_| ClamAvConfigError::InvalidPort)?,
            _ => 3310,
        };
        Ok(Self::new(ClamdEndpoint::Tcp { host, port }))
    }
}

/// Parsed `clamd` INSTREAM reply (unit-testable; no live daemon).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClamdResponse {
    /// Clean stream.
    Ok,
    /// Infection found (signature name discarded for Display safety).
    Found,
    /// Daemon reported ERROR.
    Error,
    /// Unrecognized reply.
    Unknown,
}

/// Parse a clamd INSTREAM response line (for example `stream: OK`).
#[must_use]
pub fn parse_clamd_response(raw: &str) -> ClamdResponse {
    let line = raw.trim().trim_matches('\0').trim();
    let body = line
        .strip_prefix("stream:")
        .or_else(|| line.strip_prefix("STREAM:"))
        .unwrap_or(line)
        .trim()
        .trim_matches('\0')
        .trim();
    if body.eq_ignore_ascii_case("OK") {
        return ClamdResponse::Ok;
    }
    let upper = body.to_ascii_uppercase();
    if upper.ends_with("FOUND") {
        return ClamdResponse::Found;
    }
    if upper.contains("ERROR") {
        return ClamdResponse::Error;
    }
    ClamdResponse::Unknown
}

async fn instream_scan<S>(mut stream: S, bytes: &[u8]) -> Result<ScanVerdict, ScanError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    // z-prefix: null-terminated command.
    stream
        .write_all(b"zINSTREAM\0")
        .await
        .map_err(|e| ScanError::scanner(Some("write_cmd"), e))?;

    const CHUNK: usize = 2048;
    let mut offset = 0;
    while offset < bytes.len() {
        let end = (offset + CHUNK).min(bytes.len());
        let chunk = &bytes[offset..end];
        let len = u32::try_from(chunk.len()).map_err(|_| ScanError::scanner_msg("chunk_len"))?;
        stream
            .write_all(&len.to_be_bytes())
            .await
            .map_err(|e| ScanError::scanner(Some("write_len"), e))?;
        stream
            .write_all(chunk)
            .await
            .map_err(|e| ScanError::scanner(Some("write_data"), e))?;
        offset = end;
    }
    stream
        .write_all(&0u32.to_be_bytes())
        .await
        .map_err(|e| ScanError::scanner(Some("write_eof"), e))?;
    stream
        .flush()
        .await
        .map_err(|e| ScanError::scanner(Some("flush"), e))?;

    let mut buf = Vec::with_capacity(128);
    let mut tmp = [0u8; 256];
    loop {
        let n = stream
            .read(&mut tmp)
            .await
            .map_err(|e| ScanError::scanner(Some("read"), e))?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.contains(&b'\0') || buf.contains(&b'\n') {
            break;
        }
        if buf.len() > 4096 {
            break;
        }
    }
    let raw = String::from_utf8_lossy(&buf);
    match parse_clamd_response(&raw) {
        ClamdResponse::Ok => Ok(ScanVerdict::Clean),
        ClamdResponse::Found => Ok(ScanVerdict::Infected {
            reason_class: Some("clamav"),
        }),
        ClamdResponse::Error => Err(ScanError::scanner_msg("clamd_error")),
        ClamdResponse::Unknown => Err(ScanError::scanner_msg("clamd_unknown")),
    }
}

#[async_trait]
impl VirusScanner for ClamAvScanner {
    async fn scan(&self, bytes: &[u8]) -> Result<ScanVerdict, ScanError> {
        match &self.endpoint {
            ClamdEndpoint::Unix(path) => {
                let stream = UnixStream::connect(path)
                    .await
                    .map_err(|e| ScanError::scanner(Some("connect_unix"), e))?;
                instream_scan(stream, bytes).await
            }
            ClamdEndpoint::Tcp { host, port } => {
                let stream = TcpStream::connect((host.as_str(), *port))
                    .await
                    .map_err(|e| ScanError::scanner(Some("connect_tcp"), e))?;
                instream_scan(stream, bytes).await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::DuplexStream;

    #[test]
    fn parse_ok_found_error() {
        assert_eq!(parse_clamd_response("stream: OK"), ClamdResponse::Ok);
        assert_eq!(parse_clamd_response("stream: OK\0"), ClamdResponse::Ok);
        assert_eq!(
            parse_clamd_response("stream: Win.Test.EICAR FOUND"),
            ClamdResponse::Found
        );
        assert_eq!(parse_clamd_response("stream: ERROR"), ClamdResponse::Error);
        assert_eq!(parse_clamd_response("nope"), ClamdResponse::Unknown);
        // Display path must not leak signature strings via ScanVerdict.
        let v = ScanVerdict::Infected {
            reason_class: Some("clamav"),
        };
        assert!(!v.to_string().contains("EICAR"));
    }

    #[tokio::test]
    async fn fake_socket_instream_ok() {
        let (client, mut server) = tokio::io::duplex(4096);
        let server_task = tokio::spawn(async move {
            // Read null-terminated zINSTREAM command.
            let mut cmd = Vec::new();
            let mut b = [0u8; 1];
            loop {
                if server.read_exact(&mut b).await.is_err() {
                    return;
                }
                cmd.push(b[0]);
                if b[0] == 0 {
                    break;
                }
            }
            loop {
                let mut len_buf = [0u8; 4];
                if server.read_exact(&mut len_buf).await.is_err() {
                    break;
                }
                let len = u32::from_be_bytes(len_buf);
                if len == 0 {
                    break;
                }
                let mut discard = vec![0u8; len as usize];
                let _ = server.read_exact(&mut discard).await;
            }
            let _ = server.write_all(b"stream: OK\0").await;
        });
        let verdict = instream_scan(client, b"hello").await.unwrap();
        assert_eq!(verdict, ScanVerdict::Clean);
        server_task.await.unwrap();
        let _ = std::mem::size_of::<DuplexStream>();
    }

    #[tokio::test]
    async fn fake_socket_instream_found() {
        let (client, mut server) = tokio::io::duplex(4096);
        let server_task = tokio::spawn(async move {
            let mut b = [0u8; 1];
            loop {
                if server.read_exact(&mut b).await.is_err() {
                    return;
                }
                if b[0] == 0 {
                    break;
                }
            }
            loop {
                let mut len_buf = [0u8; 4];
                if server.read_exact(&mut len_buf).await.is_err() {
                    break;
                }
                let len = u32::from_be_bytes(len_buf);
                if len == 0 {
                    break;
                }
                let mut discard = vec![0u8; len as usize];
                let _ = server.read_exact(&mut discard).await;
            }
            let _ = server
                .write_all(b"stream: Eicar-Test-Signature FOUND\0")
                .await;
        });
        let verdict = instream_scan(client, b"X5O!").await.unwrap();
        assert!(matches!(verdict, ScanVerdict::Infected { .. }));
        assert!(!verdict.to_string().contains("Eicar"));
        server_task.await.unwrap();
    }
}
