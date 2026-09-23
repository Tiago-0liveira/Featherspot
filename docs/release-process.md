# Release process

A passing Windows CI job for a push to `main` triggers the Windows release workflow. Pull requests
and other branches do not publish a release. Linux and macOS checks still run, but do not block
the Windows release. Release jobs wait for earlier main CI runs, then build the exact commit that
passed Windows CI.

The workflow chooses a version above the highest `vMAJOR.MINOR.PATCH` tag, starting above `0.1.0`.
It builds `mellowdeck-cli.exe` and `mellowdeck-player-host.exe` together and publishes a per-user
MSI, portable ZIP, `install.ps1`, `install.cmd`, SHA-256 checksums, and separate CycloneDX SBOMs
for both executables. Published releases are public and marked unsigned. A retry of an already
published commit does not produce another release.

The MSI owns both executables and the user PATH entry. Its upgrade identity and install path stay
stable across versions, while settings, cache, and logs remain in `%LOCALAPPDATA%\Mellowdeck`.
Running either install script again installs the latest MSI. A future in-app updater can use the
same release asset after the CLI exits.

The earlier draft MSI used a machine-wide install location. Users who installed that draft should
uninstall it once before using the per-user installer, so Windows does not retain two installations.

Before promoting a release as stable, complete the Windows feasibility matrix in
`docs/feasibility-checklist.md`, review keyboard and accessibility behavior, and run live Spotify
checks with an allowlisted Premium test account. Code signing and a dependency/license report
remain separate release-readiness work.
