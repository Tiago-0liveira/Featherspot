# lspotify

> A calm, artwork-led terminal Spotify client for Windows, built in Rust.

lspotify is an experimental, non-commercial, open-source Spotify terminal client. It combines
a clean, testable domain core with a rich Ratatui terminal interface, keeping Spotify Web API,
storage, and platform concerns behind explicit boundaries.

> [!IMPORTANT]
> lspotify is not affiliated with or endorsed by Spotify. A Spotify Premium account is
> required for playback control. Spotify Development Mode currently restricts new applications to
> allowlisted users; public sign-in requires Spotify approval.

## What works today

- Guided onboarding with a user-supplied Spotify Client ID
- Authorization Code with PKCE using a loopback callback on `127.0.0.1:43821`
- Secure refresh-token storage in Windows Credential Manager
- Session restoration and signed-in profile loading
- Personal Home collections, catalog search, and Spotify artwork (half-block or terminal graphics)
- Spotify Connect device selection and queue display
- Play, pause, previous, next, seek, shuffle, repeat, and volume controls
- Keyboard navigation, mouse clicks/scrolling, contextual help, and responsive terminal layouts

Playback is controlled across your devices via Spotify Connect.

## Project shape

The repository is a Cargo workspace centered around `lspotify-cli`:

```text
crates/
├── lspotify-cli          Terminal interface (Ratatui TUI, views, artwork, key/mouse dispatch)
├── lspotify-core         Domain models, validated IDs, state, and settings
├── lspotify-spotify      OAuth PKCE, Web API calls, policy, and callback server
├── lspotify-storage      JSON settings, session persistence, and SQLite cache
├── lspotify-platform     Windows/platform capabilities, WebView2 player bridge, and credentials
├── lspotify-player-host  WebView2 player-host process for local Web Playback SDK
└── lspotify-playback     Player-host IPC protocol and target coordination
```

## Getting started

### Prerequisites

- Windows (Windows 10/11 x64) with Microsoft Edge WebView2 Evergreen Runtime
- A Spotify Premium account for playback
- A Spotify Developer application in Development Mode

### Install on Windows

In PowerShell, run this one command to download and silently install the latest release:

```powershell
irm https://github.com/Tiago-0liveira/lspotify/releases/latest/download/install.ps1 | iex
```

Or [download `install.cmd`](https://github.com/Tiago-0liveira/lspotify/releases/latest/download/install.cmd)
and run it. You can also [download the MSI directly](https://github.com/Tiago-0liveira/lspotify/releases/latest).
The installer is per-user, requires no administrator rights or Rust toolchain, and puts
`lspotify` on your PATH. Open a new terminal after installation and run:

```powershell
lspotify
```

Both the CLI and its player host are installed together. Running the install command again installs
the latest version over an older one; close lspotify first. `lspotify --version` shows the
installed release version. The portable ZIP on the Releases page includes both executables but
does not add them to PATH.

### Configure Spotify

1. Create an application in the [Spotify Developer Dashboard](https://developer.spotify.com/dashboard).
2. Add this exact redirect URI: `http://127.0.0.1:43821/callback`.
3. Add your Spotify account as a test user if Development Mode requires it.
4. Start lspotify and enter the public Client ID during onboarding.

lspotify uses Authorization Code with PKCE/S256. No client secret is needed, and no client
secret should be placed in this repository or in a desktop binary. The callback verifies a
cryptographically random state value, accepts one matching callback, and then shuts down.

For full setup notes, see [`docs/spotify-setup.md`](docs/spotify-setup.md).

### Build and run

To build from source, install Rust `1.98.1` (the pinned toolchain in `rust-toolchain.toml`).

```powershell
cargo build -p lspotify-player-host
cargo run
```

The CLI launches `lspotify-player-host.exe` from the same Cargo output directory. Build the
host once before running the CLI, and rebuild it after changing the player-host crate.

The terminal client adapts from 80 columns upward:
- Wide terminals show Navigation, Browse, and Queue together.
- Medium terminals open Queue as a page.
- Compact terminals use the `☰ Menu` overlay.

**Controls**:
- `Tab` / `Shift+Tab`: Move focus between panes
- Arrows or `j` / `k`: Move within a pane
- `Enter`: Open or play selection
- `/`: Search
- `a`: Append track to queue
- `Shift+Left` / `Shift+Right`: Seek backward/forward
- `?`: Searchable contextual help
- Mouse clicks, double-clicks, right-click actions, and wheel scrolling are supported.
- Artwork mode (`Auto`, `Blocks`, or `Off`), mouse input, and layout breakpoints can be configured on the Settings page.

## Development checks

Run the standard workspace checks before opening a change:

```powershell
cargo fmt --all --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

See [Contributing](CONTRIBUTING.md) to enable the pre-commit hook.

## Privacy and local data

Refresh tokens are stored through Windows Credential Manager. Local settings and session data are
kept in `%LOCALAPPDATA%\lspotify`.

lspotify does not log access tokens, refresh tokens, Client IDs, or search text. See
[`PRIVACY.md`](PRIVACY.md) for the project privacy statement.

## License

Source code is available under your choice of the [MIT License](LICENSE-MIT) or
[Apache License 2.0](LICENSE-APACHE). Spotify services, trademarks, and content are governed
separately by Spotify's terms and policies.
