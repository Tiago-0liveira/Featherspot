# Milestone 0 feasibility checklist

- [ ] GPUI creates a Windows 10/11 x64 window and exposes the native handle.
- [ ] A WebView2 child is created, resized, and torn down safely.
- [ ] The Spotify Web Playback SDK initializes for a Premium allowlisted account.
- [ ] Versioned Rust↔JavaScript IPC rejects malformed and unknown-version messages.
- [ ] Registration, explicit transfer, pause, seek, and volume work.
- [ ] Startup timeout and player failures fall back cleanly to Spotify Connect.
- [ ] GPUI exposes usable semantics to Windows Narrator.
- [ ] Adaptive refresh behaves on 60 Hz and high-refresh displays.

If protected playback is unreliable in WebView2, v1 ships as a Connect controller while preserving
the `LocalPlayerHost` port. An unofficial playback engine is not an acceptable fallback.

