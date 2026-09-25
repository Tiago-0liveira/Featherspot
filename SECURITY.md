# Security

Please do not publish credentials or vulnerability details in a normal issue.

Use GitHub private vulnerability reporting when it is available for this repository. If it is not
available, open a minimal issue asking the maintainer for a private contact channel **without**
including exploit details.

Never include Spotify access tokens, refresh tokens, OAuth authorization codes/verifiers, private
account identifiers, client secrets, or unredacted diagnostic logs.

Security fixes should preserve the existing rule that tokens are never written to application
logs. Only the latest release is actively supported.
