# Contributing

Use focused changes and keep `mellowdeck-core` free of UI, HTTP, database, WebView, and OS
dependencies. New user-visible strings must be added to both Fluent locale files. New Spotify
response models must be converted to domain models at the adapter boundary.

Before opening a change, run formatting, strict Clippy, and the full workspace test suite. Live
Spotify tests must remain opt-in and must never run for untrusted pull requests.

