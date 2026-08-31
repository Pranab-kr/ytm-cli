#!/usr/bin/env bash
# Build a release tarball for GitHub Releases.
#
# Produces dist/ytm-cli-<version>-<arch>-<os>.tar.gz holding the binary, the
# README, the example config, and a checksum. Run the gate first — a release
# built from a red tree is worse than no release.
#
#   ./scripts/release.sh              # build the tarball
#   ./scripts/release.sh --publish    # also create the GitHub release (needs gh)
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=$(grep -m1 '^version' crates/ytm-cli/Cargo.toml | cut -d'"' -f2)
ARCH=$(uname -m)
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
NAME="ytm-cli-v${VERSION}-${ARCH}-${OS}"

echo "==> gate"
./scripts/check.sh

echo "==> building release"
cargo build --release

echo "==> packaging ${NAME}"
rm -rf "dist/${NAME}"
mkdir -p "dist/${NAME}"
cp target/release/ytm-cli "dist/${NAME}/"
cp README.md config.example.toml "dist/${NAME}/"
# The binary links libmpv at runtime, so say so where someone will read it.
cat > "dist/${NAME}/INSTALL.txt" <<'TXT'
ytm-cli — YouTube Music in your terminal

Runtime requirements (not bundled):
  mpv / libmpv   audio playback
  yt-dlp         stream resolution — keep it current, YouTube breaks it often

  Arch:    sudo pacman -S mpv yt-dlp
  Debian:  sudo apt install libmpv2 yt-dlp
  macOS:   brew install mpv yt-dlp

Install:
  cp ytm-cli ~/.local/bin/          # or anywhere on your PATH
  ytm-cli config                    # write and edit config.toml
  ytm-cli playlists                 # check auth without the TUI
  ytm-cli                           # go

Set up cookie auth first — see the "Authentication" section of README.md.
TXT

tar -czf "dist/${NAME}.tar.gz" -C dist "${NAME}"
rm -rf "dist/${NAME}"
(cd dist && sha256sum "${NAME}.tar.gz" > "${NAME}.tar.gz.sha256")

echo
echo "built: dist/${NAME}.tar.gz"
du -h "dist/${NAME}.tar.gz" | cut -f1
cat "dist/${NAME}.tar.gz.sha256"

if [[ "${1:-}" == "--publish" ]]; then
  command -v gh >/dev/null || { echo "gh CLI not installed"; exit 1; }
  git remote get-url origin >/dev/null 2>&1 || { echo "no git remote 'origin'"; exit 1; }
  echo "==> creating GitHub release v${VERSION}"
  gh release create "v${VERSION}" \
    "dist/${NAME}.tar.gz" "dist/${NAME}.tar.gz.sha256" \
    --title "ytm-cli v${VERSION}" \
    --notes "See README.md. Requires mpv and yt-dlp at runtime."
fi
