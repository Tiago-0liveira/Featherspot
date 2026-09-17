# Security policy

Please report vulnerabilities privately to the repository maintainers rather than opening a public
issue. Never submit access tokens, refresh tokens, Spotify client secrets, account identifiers, or
unredacted diagnostic logs.

Mellowdeck stores only the refresh token in the operating-system credential vault. Access tokens
remain in memory. Diagnostic logging must redact authorization headers, tokens, OAuth codes, and
state/verifier values.

