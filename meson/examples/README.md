# Meson examples

## `upload_and_load`

Creates a `ReceiptScan` File row with `create_with_bytes`, then loads bytes with
session `get` + `get_file_bytes`.

```bash
CARGO_BUILD_JOBS=1 cargo run -p meson --example upload_and_load
# expect: ok uploaded+loaded 5 bytes
```
