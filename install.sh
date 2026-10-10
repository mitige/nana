#!/bin/sh
# installs the nana binary from the latest GitHub release.
# usage: curl -fsSL https://raw.githubusercontent.com/mitige/nana/main/install.sh | sh
# the binary goes to ~/.local/bin unless NANA_INSTALL_DIR says otherwise.
set -eu

asset="nana-linux-x86_64.tar.gz"
dir="${NANA_INSTALL_DIR:-$HOME/.local/bin}"
url="https://github.com/mitige/nana/releases/latest/download/$asset"

case "$(uname -s)" in
    Linux) ;;
    *) echo "nana: this installer is for linux; on windows use install.ps1" >&2; exit 1 ;;
esac

case "$(uname -m)" in
    x86_64|amd64) ;;
    *) echo "nana: no prebuilt binary for $(uname -m); build from source with cargo install --git https://github.com/$repo" >&2; exit 1 ;;
esac

for tool in curl tar; do
    command -v "$tool" >/dev/null 2>&1 || { echo "nana: $tool is required" >&2; exit 1; }
done

tmp="$(mktemp -d)"
trap 'rm -r "$tmp"' EXIT

echo "nana: downloading $asset"
curl -fsSL "$url" -o "$tmp/$asset"
tar -xzf "$tmp/$asset" -C "$tmp"

mkdir -p "$dir"
install -m 755 "$tmp/nana" "$dir/nana"

echo "nana: installed $("$dir/nana" --version) to $dir/nana"
case ":$PATH:" in
    *":$dir:"*) ;;
    *) echo "nana: $dir is not on your PATH yet" ;;
esac
