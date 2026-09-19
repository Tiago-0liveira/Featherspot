# Mellowdeck

> A calm, artwork-led Spotify desktop client for Windows, built in Rust.

Mellowdeck is an experimental, non-commercial, open-source Spotify client. It combines a
small, testable domain core with a native desktop shell and keeps Spotify, storage, playback,
and platform concerns behind explicit interfaces.

> [!IMPORTANT]
> Mellowdeck is not affiliated with or endorsed by Spotify. A Spotify Premium account is
> required for playback. Spotify Development Mode currently restricts new applications to
> allowlisted users; public sign-in requires Spotify approval.

## What works today

- Guided onboarding with a user-supplied Spotify Client ID
- Authorization Code with PKCE using a loopback callback on `127.0.0.1:43821`
- Secure refresh-token storage in Windows Credential Manager
- Session restoration and signed-in profile loading
- Personal Home collections, catalog search, and Spotify artwork
- Spotify Connect device selection and queue display
- Play, pause, previous, next, seek, and volume controls
- A half-revealed vinyl interaction that rotates during playback and can be dragged to seek
- Compact and expanded visual themes, localization, keyboard navigation, and redacted logs

The authenticated shell and Web API integration are functional. A hardened Wry/WebView2
playback bridge is present, but embedded Web Playback SDK support remains gated on the protected-
media feasibility work. Until that work is complete, audio comes from the selected Spotify
Connect device.

## Project shape

The repository is a Cargo workspace. `mellowdeck-core` owns domain models, validated IDs,
state, actions, errors, and asynchronous ports. The remaining crates are adapters that depend
inward on that core:

```text
mellowdeck
├── mellowdeck-ui           GPUI shell, views, themes, localization, and artwork
├── mellowdeck-spotify      OAuth, Web API calls, policy, and callbacks
├── mellowdeck-playback     Playback state and local-player bridge boundary
├── mellowdeck-storage      SQLite cache, settings, and secure-token integration
├── mellowdeck-platform     Windows/platform capabilities
├── mellowdeck-player-host  WebView2 player-host process
├── mellowdeck-cli          Terminal interface for core capabilities
└── mellowdeck-test-support Shared fixtures and test helpers
```

State transitions pass through named `Action` values and `AppState::reduce`. Core does not
depend on GPUI, Wry, HTTP, SQLite, or operating-system APIs. See
[the architecture notes](docs/architecture.md) for the dependency rules and playback boundary.

## Getting started

### Prerequisites

- Windows with the WebView2 runtime available
- Rust `1.98.1` (the pinned toolchain in `rust-toolchain.toml`)
- A Spotify Premium account for playback
- A Spotify Developer application in Development Mode

### Configure Spotify

1. Create an application in the [Spotify Developer Dashboard](https://developer.spotify.com/dashboard).
2. Add this exact redirect URI: `http://127.0.0.1:43821/callback`.
3. Add your Spotify account as a test user if Development Mode requires it.
4. Start Mellowdeck and enter the public Client ID during onboarding.

Mellowdeck uses Authorization Code with PKCE/S256. No client secret is needed, and no client
secret should be placed in this repository or in a desktop binary. The callback verifies a
cryptographically random state value, accepts one matching callback, and then shuts down.

For the full setup notes, see [`docs/spotify-setup.md`](docs/spotify-setup.md).

### Build and run

```powershell
cargo run -p mellowdeck
```

The terminal client is also available while the workspace is under development:

```powershell
cargo build -p mellowdeck-player-host
cargo run -p mellowdeck-cli
```

The CLI launches `mellowdeck-player-host.exe` from the same Cargo output directory. Build the
host once before running the CLI, and rebuild it after changing the player-host crate.

The terminal client adapts from 80 columns upward. Wide terminals show Navigation, Browse, and
Queue together; medium terminals open Queue as a page; compact terminals use the `☰ Menu` overlay.
Use `Tab`/`Shift+Tab` to move focus, arrows or `j`/`k` to move within a pane, `Enter` to open or
play, `/` to search, `a` to append a track to the queue, and `?` for searchable contextual help.
Seeking uses `Shift+Left`/`Shift+Right`. Mouse clicks, double-clicks, right-click actions, wheel
scrolling, and player controls are enabled by default. CLI artwork (`Auto`, `Blocks`, or `Off`),
mouse input, and the wide-screen queue can be changed on the Settings page and are stored with the
existing local settings.

## Development checks

Run the standard workspace checks before opening a change:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Useful project documentation:

- [Architecture](docs/architecture.md)
- [UX specification](docs/ux-specification.md)
- [UI and vinyl plan](docs/ui-and-vinyl-plan.md)
- [Feasibility checklist](docs/feasibility-checklist.md)
- [Release process](docs/release-process.md)
- [Contributing](CONTRIBUTING.md)

## Privacy and local data

Refresh tokens are stored through Windows Credential Manager. Local settings and cache data are
kept on the machine, and daily redacted logs are written to
`%LOCALAPPDATA%\Mellowdeck\logs`.

Mellowdeck does not log access tokens, refresh tokens, Client IDs, or search text. See
[`PRIVACY.md`](PRIVACY.md) for the project privacy statement.

## License

Source code is available under your choice of the [MIT License](LICENSE-MIT) or
[Apache License 2.0](LICENSE-APACHE). Spotify services, trademarks, and content are governed
separately by Spotify's terms and policies.
