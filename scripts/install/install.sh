#!/usr/bin/env sh
set -eu

repo="Tiago-0liveira/lspotify"
base="https://github.com/$repo/releases/latest/download"
install_dir="${LSPOTIFY_INSTALL_DIR:-$HOME/.local/bin}"
tmp_dir="$(mktemp -d 2>/dev/null || mktemp -d -t lspotify-install)"

cleanup() {
    rm -rf "$tmp_dir"
}
trap cleanup EXIT INT TERM

os="$(uname -s)"
arch="$(uname -m)"

case "$os:$arch" in
    Darwin:x86_64)
        asset="lspotify-macos-x86_64"
        ;;
    Darwin:arm64|Darwin:aarch64)
        asset="lspotify-macos-aarch64"
        ;;
    Linux:x86_64|Linux:amd64)
        asset="lspotify-linux-x86_64"
        ;;
    *)
        echo "lspotify does not currently provide a release for $os/$arch." >&2
        exit 1
        ;;
esac

echo "Downloading $asset..."
curl -fsSL "$base/$asset" -o "$tmp_dir/$asset"
curl -fsSL "$base/SHA256SUMS.txt" -o "$tmp_dir/SHA256SUMS.txt"

expected="$(
    awk -v name="$asset" '
        $2 == name || $2 == "*" name { print $1; exit }
    ' "$tmp_dir/SHA256SUMS.txt"
)"
if [ -z "$expected" ]; then
    echo "SHA256SUMS.txt does not contain $asset." >&2
    exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "$tmp_dir/$asset" | awk '{print $1}')"
elif command -v shasum >/dev/null 2>&1; then
    actual="$(shasum -a 256 "$tmp_dir/$asset" | awk '{print $1}')"
else
    echo "No SHA-256 utility found (sha256sum or shasum)." >&2
    exit 1
fi

if [ "$actual" != "$expected" ]; then
    echo "Checksum verification failed for $asset." >&2
    exit 1
fi

mkdir -p "$install_dir"
install -m 0755 "$tmp_dir/$asset" "$install_dir/lspotify"

echo "Installed lspotify to $install_dir/lspotify"
case ":${PATH:-}:" in
    *":$install_dir:"*) ;;
    *) echo "Add $install_dir to PATH if 'lspotify' is not found in a new shell." ;;
esac
