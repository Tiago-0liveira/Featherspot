# lspotify

A lightweight Spotify client that lives in your terminal.

lspotify is an independent, non-commercial open-source project. It is **not affiliated with,
endorsed by, or supported by Spotify**. Spotify and the Spotify logo are trademarks of Spotify AB.

## Install

**Windows (PowerShell)**

```powershell
irm https://github.com/Tiago-0liveira/lspotify/releases/latest/download/install.ps1 | iex
```

**macOS / Linux (including Arch)**

```sh
curl -fsSL https://github.com/Tiago-0liveira/lspotify/releases/latest/download/install.sh | sh
```

Release installers verify the published SHA-256 checksum before installing. You can also download
release binaries directly from GitHub Releases.

## Why lspotify?

The regular Spotify desktop experience is excellent, but it is much more UI than some machines or
workflows need. lspotify keeps the interface in a terminal and can use a lightweight native
playback engine, making it a good fit for older or low-end computers, remote/keyboard-heavy
setups, and people who simply prefer terminal applications.

It supports browsing, search, library and playlist views, queue management, Spotify Connect device
selection, artwork, keyboard and mouse input, and the normal playback controls.

## Setup

You need a **Spotify Premium** account for local playback and a Spotify Developer application.

1. Create an app at the [Spotify Developer Dashboard](https://developer.spotify.com/dashboard).
2. Add this redirect URI exactly: `http://127.0.0.1:43821/callback`.
3. If Spotify Development Mode asks for test users, add the Spotify account you will sign in with.
4. Run `lspotify` and enter the app's **Client ID** when asked.
5. Complete Spotify sign-in in your browser.

lspotify uses Authorization Code with PKCE. **Do not create, paste, or ship a client secret.**

## Playback engines

lspotify can always control other Spotify Connect devices. Local playback is started **only when
you choose “This computer”** from Devices.

| Platform | Local engine | Notes |
| --- | --- | --- |
| Windows | Spotify Web Playback SDK (default) | Official Spotify playback technology; uses WebView2 and more memory while active. |
| Windows | Librespot (optional) | Lightweight native Rust playback; unofficial and not supported by Spotify. |
| macOS | Librespot | Lightweight native local playback. |
| Linux | Librespot | Lightweight native local playback. |

On the first Windows run, lspotify asks which local engine you want. You can change it later in
**Settings → Playback**. On macOS and Linux the setting is shown but disabled because Librespot is
the only local engine currently available there.

Librespot is an independent reverse-engineered implementation of Spotify playback. It can stop
working when Spotify changes private protocols. If local playback is unavailable, lspotify can
still act as a terminal controller for another Spotify Connect device.

## Controls

Press `?` inside lspotify for the complete contextual shortcut list. The essentials are:

- `Enter` open/play, `Space` play/pause, `d` devices, `/` search
- arrows or `j`/`k` navigate
- `a` add a track to the queue
- `Shift+Left` / `Shift+Right` seek
- `Q`, `Ctrl+C`, or `Ctrl+Q` quit

Keyboard bindings can be changed in Settings.

## Build from source

The repository pins its Rust toolchain in `rust-toolchain.toml`.

```sh
cargo build --workspace
cargo run -p lspotify-cli
```

On Linux, the Rodio/CPAL audio backend requires the normal ALSA development package (for example
`libasound2-dev` on Debian/Ubuntu or `alsa-lib` on Arch).

## Privacy and legal

lspotify has no telemetry, advertising, analytics, or lspotify-operated backend. See
[PRIVACY.md](PRIVACY.md) for what is stored locally.

Using Spotify through lspotify is still subject to Spotify's terms. The official Windows playback
backend uses Spotify's documented Web Playback SDK; Librespot is a separate unofficial project.
See [TERMS.md](TERMS.md) and [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

## Contributing

Help is welcome. Bug fixes, platform testing, cleanup, documentation, new ideas, and feature
proposals are all useful. Open an issue or pull request even if you are not sure where the change
belongs; [CONTRIBUTING.md](CONTRIBUTING.md) has the development checks and a few architectural
guidelines.

## Credits

A special thank-you to [librespot](https://github.com/librespot-org/librespot) and its contributors
for the lightweight Spotify Connect/playback implementation used by lspotify on macOS and Linux
and offered as an option on Windows.

lspotify also builds on excellent Rust projects including
[Ratatui](https://github.com/ratatui/ratatui), Crossterm, Wry/WebView2, Rodio/CPAL, Reqwest, and
Rusqlite.

## License

lspotify's own source code is licensed under your choice of
[MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE). Third-party components keep their own licenses;
see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
