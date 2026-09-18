//! Lab AWS SigV4 (S3) signing for RustFS / S3-compatible HTTP.
//!
//! Local copy of the Nucleus lab SigV4 helper (Meson has no Nucleus dependency).

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// Sign an HTTP request for RustFS / S3-compatible APIs.
///
/// Returns `(authorization, amz_date, content_sha256)` header values.
#[must_use]
pub fn sign_s3_request(
    method: &str,
    url: &str,
    body: &[u8],
    access_key: &str,
    secret_key: &str,
    region: &str,
    extra_headers: &[(&str, &str)],
) -> (String, String, String) {
    let (host, path, query) = split_url(url);
    let now = chrono::Utc::now();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let datestamp = now.format("%Y%m%d").to_string();
    let payload_hash = hex::encode(Sha256::digest(body));
    let service = "s3";

    let mut headers: Vec<(String, String)> = vec![
        ("host".into(), host),
        ("x-amz-content-sha256".into(), payload_hash.clone()),
        ("x-amz-date".into(), amz_date.clone()),
    ];
    for (k, v) in extra_headers {
        headers.push((k.to_ascii_lowercase(), (*v).to_string()));
    }
    headers.sort_by(|a, b| a.0.cmp(&b.0));
    let signed_headers = headers
        .iter()
        .map(|(k, _)| k.as_str())
        .collect::<Vec<_>>()
        .join(";");
    let mut canonical_headers = String::new();
    for (k, v) in &headers {
        use std::fmt::Write as _;
        let _ = writeln!(canonical_headers, "{k}:{}", v.trim());
    }
    let canonical_request =
        format!("{method}\n{path}\n{query}\n{canonical_headers}\n{signed_headers}\n{payload_hash}");
    let credential_scope = format!("{datestamp}/{region}/{service}/aws4_request");
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{credential_scope}\n{}",
        hex::encode(Sha256::digest(canonical_request.as_bytes()))
    );
    let signing_key = aws4_signing_key(secret_key, &datestamp, region, service);
    let signature = hex::encode(hmac_sha256(&signing_key, string_to_sign.as_bytes()));
    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={access_key}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}"
    );
    (authorization, amz_date, payload_hash)
}

fn split_url(url: &str) -> (String, String, String) {
    let without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let (authority, rest) = match without_scheme.find('/') {
        Some(i) => (&without_scheme[..i], &without_scheme[i..]),
        None => (without_scheme, "/"),
    };
    let (path, query) = match rest.find('?') {
        Some(i) => (rest[..i].to_string(), rest[i + 1..].to_string()),
        None => (rest.to_string(), String::new()),
    };
    let path = if path.is_empty() {
        "/".to_string()
    } else {
        path
    };
    (authority.to_string(), path, query)
}

fn aws4_signing_key(secret: &str, datestamp: &str, region: &str, service: &str) -> Vec<u8> {
    let k_date = hmac_sha256(format!("AWS4{secret}").as_bytes(), datestamp.as_bytes());
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, service.as_bytes());
    hmac_sha256(&k_service, b"aws4_request")
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_s3_request_stable_shape() {
        let (auth, amz, hash) = sign_s3_request(
            "PUT",
            "http://127.0.0.1:9000/bucket/key",
            b"hello",
            "mesonlabaccess",
            "mesonlabsecret",
            "us-east-1",
            &[("content-type", "application/octet-stream")],
        );
        assert!(auth.starts_with("AWS4-HMAC-SHA256 Credential=mesonlabaccess/"));
        assert!(auth.contains("SignedHeaders="));
        assert!(auth.contains("Signature="));
        assert_eq!(amz.len(), 16);
        assert_eq!(hash.len(), 64);
    }
}
