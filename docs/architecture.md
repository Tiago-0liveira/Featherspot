# Architecture

The executable is the composition root. `lspotify-core` owns domain models, validated IDs,
state, actions, errors, and object-safe asynchronous ports. Every other crate is an adapter that
depends inward on core. Core must not depend on Ratatui, Wry, HTTP, SQLite, or OS APIs.

```text
lspotify
 ├── lspotify-cli ─────────→ lspotify-core
 ├── lspotify-spotify ────→ lspotify-core
 ├── lspotify-playback ───→ lspotify-core
 ├── lspotify-storage ────→ lspotify-core
 └── lspotify-platform ───→ lspotify-core
```

State changes pass through named `Action` values and `AppState::reduce`. UI entities should
subscribe to the narrowest state slice. Adapters return domain values, never vendor DTOs.

The local-player boundary uses versioned JSON envelopes. Spotify Web Playback SDK events are
authoritative for the embedded device; Web API responses are authoritative for Connect devices.
The reconciler interpolates progress only while playing and clamps it to the known duration.

Platform code is selected behind `cfg(target_os)` and stable core traits. Any necessary `unsafe`
must be isolated in a platform-specific module with a documented safety invariant.
