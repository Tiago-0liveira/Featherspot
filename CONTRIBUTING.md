# Contributing

Contributions are welcome: bug fixes, platform testing, documentation, UI/UX improvements, new
ideas, and focused refactors are all useful.

Keep domain logic in `lspotify-core`; Spotify/Web API, storage, playback-engine, UI, and
OS-specific concerns should stay behind their existing crate boundaries. Please avoid adding
secrets, access tokens, refresh tokens, account identifiers, or unredacted logs to issues/tests.

Before opening a pull request, run:

```sh
cargo fmt --all --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

On Linux, install the ALSA development headers required by CPAL/Rodio first.

You can enable the repository's pre-commit checks once per clone:

```sh
git config core.hooksPath .githooks
```

Small PRs are easier to review, but early proposals are welcome when you want feedback before
writing the implementation.
