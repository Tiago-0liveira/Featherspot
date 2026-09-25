# Privacy

lspotify has **no telemetry, advertising, analytics, or lspotify-operated server**. The application
talks directly to Spotify and GitHub (for update checks).

Locally, lspotify stores settings, UI/session preferences, cached Spotify metadata/artwork, and
diagnostic logs under the platform's normal per-user application-data location. On Windows, the
Spotify refresh token is stored in Windows Credential Manager. Access tokens remain in memory.

The embedded Librespot backend is created without Librespot credential or audio caching: lspotify
passes the current short-lived Spotify access token to the in-process session and does not ask
Librespot to persist reusable credentials or downloaded audio.

Logging out removes the stored refresh token where persistent credential storage is available.
Clearing the cache removes cached Spotify metadata/artwork. lspotify does not intentionally log
OAuth tokens, authorization codes, PKCE verifiers, or search text.

Spotify itself processes account, playback, and catalog requests according to Spotify's own
privacy terms. GitHub processes release/update requests according to GitHub's terms.
