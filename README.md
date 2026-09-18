# Meson

[![CI](https://github.com/unified-field-dev/meson/actions/workflows/ci.yml/badge.svg)](https://github.com/unified-field-dev/meson/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

[GitHub](https://github.com/unified-field-dev/meson) ·
`cargo doc -p meson --features backend-local --open`

Valence **File** trait and byte backends for Unified Field uploads. Metadata
lives in Valence; bytes live behind a host-installed `FileByteBackend`. Product
code uses `create_with_bytes` and `get_file_bytes`.

```rust,ignore
let created = ReceiptScan::create_with_bytes(
    system_v,
    FileCreateMeta {
        file_name: "receipt-1.png".into(),
        file_extension: "png".into(),
        mime_type: "image/png".into(),
        uploaded_by: owner,
    },
    b"PNG..",
)
.await?;
let bare = created.id().ok_or("id")?.id().to_string();
let row = ReceiptScan::get(&bare, session_v).await?.ok_or("not found")?;
Ok(row.get_file_bytes().await?)
```

## About

- **File trait** — shared upload metadata. Opt in with `traits: [File]`.
- **File upload / File bytes** — `create_with_bytes` then session `get` +
  `get_file_bytes` (host installs the store once).
- **Byte backends** — `LocalDiskBlobStore` or `RustFsBlobStore` (`backend-rustfs`).
- **Env selection** — `blob_store_from_env()` + `install_blob_store()`.
- **Owner-scoped queries** — `FileQueryAll` + `find_file_in_owned_rows` for My Files.

Consuming crates must **vendor** `schemas/file_valence_trait.rs` into their own
`schemas/` directory. Valence codegen only merges traits from the local tree.

Crate-root rustdoc owns the Features index and get-started guides
(`cargo doc -p meson --features backend-local,backend-rustfs --open`).

## Feature flags

| Flag | Purpose |
|------|---------|
| `db-sqlite` (default) | Valence sqlite backend |
| `db-hybrid` | Valence hybrid backend |
| `backend-local` (default) | `LocalDiskBlobStore` |
| `backend-rustfs` | `RustFsBlobStore` (RustFS / S3 path-style) |

Runtime pick: `MESON_BLOB_BACKEND=local|rustfs` (see crate rustdoc
“Select blob store from env”).

## Getting started

```toml
[dependencies]
# Pin a tag or commit SHA — do not use branch = "main".
meson = { git = "https://github.com/unified-field-dev/meson", package = "meson", rev = "<tag-or-sha>", default-features = false, features = ["db-sqlite", "backend-local"] }
```

1. Vendor `file_valence_trait.rs` under the consuming crate’s `schemas/`.
2. On a concrete `valence_schema!`, set `traits: [File]` and rebuild.
3. At host boot: `install_blob_store(blob_store_from_env()?)`.
4. Upload under System Valence with `YourFile::create_with_bytes`.
5. Load under session Valence with `YourFile::get` + `get_file_bytes`.

```rust,ignore
use meson::{FileBytes, FileCreateMeta, FileUpload, ReceiptScan};
use valence::{Model, RecordId};

// Host boot (once):
// meson::install_blob_store(meson::blob_store_from_env()?);

async fn add_and_load(
    system_v: &valence::Valence,
    session_v: &valence::Valence,
    owner: RecordId,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let created = ReceiptScan::create_with_bytes(
        system_v,
        FileCreateMeta {
            file_name: "receipt-1.png".into(),
            file_extension: "png".into(),
            mime_type: "image/png".into(),
            uploaded_by: owner,
        },
        b"PNG..",
    )
    .await?;
    let bare = created.id().ok_or("id")?.id().to_string();
    let row = ReceiptScan::get(&bare, session_v).await?.ok_or("not found")?;
    Ok(row.get_file_bytes().await?)
}
```

## Examples

```bash
CARGO_BUILD_JOBS=1 cargo run -p meson --example upload_and_load
# expect: ok uploaded+loaded 5 bytes
```

## Verify

```bash
export CARGO_BUILD_JOBS=1
cargo test -p meson --features backend-local
cargo run -p meson --example upload_and_load
```

Full local + RustFS gates and monorepo host e2e:
[`docs/VERIFICATION.md`](docs/VERIFICATION.md). GitHub Actions runs the CI
subset on every PR and push to `main`.

## License

MIT. See [LICENSE](LICENSE), [CONTRIBUTING.md](CONTRIBUTING.md),
[SECURITY.md](SECURITY.md), and [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
