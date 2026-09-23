# Contributing

Use focused changes and keep `lspotify-core` free of UI, HTTP, database, WebView, and OS
dependencies. New user-visible strings must be added to both Fluent locale files. New Spotify
response models must be converted to domain models at the adapter boundary.

Before opening a change, run formatting, strict Clippy, and the full workspace test suite. Live
Spotify tests must remain opt-in and must never run for untrusted pull requests.

Install the tracked pre-commit hook once per clone:

```sh
git config core.hooksPath .githooks
```

The hook checks the working tree with `cargo fmt --all --check`,
`cargo check --workspace --all-targets --locked`, and strict Clippy before each commit.
Run `cargo fmt --all` to apply formatting if the first check fails. The hook uses the
Rust toolchain specified in `rust-toolchain.toml`. Run `cargo test --workspace --locked`
before opening a pull request.

