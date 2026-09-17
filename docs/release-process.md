# Release process

1. Complete the Windows feasibility matrix in `docs/feasibility-checklist.md`.
2. Run formatting, Clippy, tests, dependency audit, and license checks.
3. Review both locales, both themes, keyboard navigation, Narrator, and high contrast.
4. Produce x64 MSI and portable ZIP artifacts on the tagged Windows workflow.
5. Publish SHA-256 checksums, SBOM, dependency/license report, and release notes.

Artifacts are marked unsigned until a signing certificate is configured. Live Spotify tests are
manual and use an allowlisted Premium test account.

