# Contributing to Meson

Thank you for improving this project.

## Development setup

1. Clone [unified-field-dev/meson](https://github.com/unified-field-dev/meson)
2. Install Rust stable
3. From the repository root:

```bash
export CARGO_BUILD_JOBS=1
cargo fmt --all -- --check
cargo clippy -p meson --all-targets --features backend-local -- -D warnings
cargo test -p meson --features backend-local
```

How to re-run checks (including RustFS): [`docs/VERIFICATION.md`](docs/VERIFICATION.md).

## Code of conduct

Participation is governed by [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md). Security
reports: [`SECURITY.md`](SECURITY.md).

## Pull requests

- Prefer small, focused PRs.
- Update [`README.md`](README.md) when public API or host wiring steps change.
