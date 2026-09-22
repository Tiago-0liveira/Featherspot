Mellowdeck portable build

Run mellowdeck-cli.exe from this directory (it launches mellowdeck-player-host.exe
for local Web Playback SDK support). Spotify playback on this device requires the Microsoft
Edge WebView2 Evergreen Runtime and Spotify Premium. If diagnostics report WebView2 missing,
install the Evergreen Runtime from:
https://developer.microsoft.com/microsoft-edge/webview2/

This build stores its settings, cache, and logs in %LOCALAPPDATA%\Mellowdeck. No offline audio is
stored. This artifact is unsigned unless the release notes explicitly say otherwise.

