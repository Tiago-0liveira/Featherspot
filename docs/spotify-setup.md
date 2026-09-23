# Spotify setup

1. Create an application in the Spotify developer dashboard.
2. Add the exact redirect URI `http://127.0.0.1:43821/callback`.
3. Set your public Client ID in lspotify onboarding. Do not create or embed a client secret.
4. Add test users in the dashboard when the application is in Development Mode.
5. Sign in. Spotify currently requires the owner of a Development Mode app to have Premium, and
   Premium is also required for embedded playback.

Authorization uses Authorization Code with PKCE/S256. The callback listener binds only to
`127.0.0.1:43821`, checks a cryptographically random state, accepts one matching callback, and
then shuts down. The `user-read-email` scope is requested only because the Web Playback SDK requires it; lspotify never reads or stores the address.

The centrally compiled Client ID is optional. Personal builds remain usable with a user-supplied
Client ID while public plain-login distribution waits for Spotify approval.
