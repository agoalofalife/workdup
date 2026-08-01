#!/bin/sh
# workdup installer — detects OS/arch and fetches the matching release binary.
#
#   curl -fsSL https://raw.githubusercontent.com/agoalofalife/workdup/main/install.sh | sh
#   WORKDUP_VERSION=v0.2.0 sh install.sh
#   WORKDUP_INSTALL_DIR=~/.local/bin sh install.sh
#
# In k8s we run the container image, not this script — this is for developer
# machines and CI runners (e.g. `workdup validate` on a rendered config).
set -eu

REPO="${WORKDUP_REPO:-agoalofalife/workdup}"
VERSION="${WORKDUP_VERSION:-latest}"
INSTALL_DIR="${WORKDUP_INSTALL_DIR:-/usr/local/bin}"

die() { echo "install.sh: $*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"; }

need curl
need tar

os=$(uname -s)
arch=$(uname -m)

case "$os" in
  Linux)  os_part="unknown-linux-gnu" ;;
  Darwin) os_part="apple-darwin" ;;
  *)      die "unsupported OS: $os (supported: Linux, Darwin)" ;;
esac

case "$arch" in
  x86_64|amd64)  arch_part="x86_64" ;;
  arm64|aarch64) arch_part="aarch64" ;;
  *)             die "unsupported architecture: $arch (supported: x86_64, arm64)" ;;
esac

target="${arch_part}-${os_part}"

# Resolve "latest" without jq: the releases/latest API always carries tag_name.
if [ "$VERSION" = "latest" ]; then
  VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | sed -n 's/.*"tag_name" *: *"\([^"]*\)".*/\1/p' | head -n1)
  [ -n "$VERSION" ] || die "could not resolve the latest release tag for ${REPO}"
fi

asset="workdup-${VERSION}-${target}.tar.gz"
base="https://github.com/${REPO}/releases/download/${VERSION}"

echo "==> workdup ${VERSION} (${target})"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM

curl -fsSL --retry 3 -o "${tmp}/${asset}" "${base}/${asset}" \
  || die "download failed: ${base}/${asset} (does this release publish ${target}?)"
curl -fsSL --retry 3 -o "${tmp}/SHA256SUMS" "${base}/SHA256SUMS" \
  || die "download failed: ${base}/SHA256SUMS"

# Checksum verification is not optional — this script is meant to be piped to sh.
if command -v sha256sum >/dev/null 2>&1; then
  sha_cmd="sha256sum"
elif command -v shasum >/dev/null 2>&1; then
  sha_cmd="shasum -a 256"
else
  die "neither sha256sum nor shasum available; cannot verify the download"
fi

expected=$(grep " ${asset}\$" "${tmp}/SHA256SUMS" | awk '{print $1}')
[ -n "$expected" ] || die "${asset} is not listed in SHA256SUMS"
actual=$(cd "$tmp" && $sha_cmd "$asset" | awk '{print $1}')
[ "$expected" = "$actual" ] || die "checksum mismatch for ${asset}: expected ${expected}, got ${actual}"

tar -xzf "${tmp}/${asset}" -C "$tmp"
[ -f "${tmp}/workdup" ] || die "archive did not contain a 'workdup' binary"
chmod +x "${tmp}/workdup"

if [ -w "$INSTALL_DIR" ]; then
  mv "${tmp}/workdup" "${INSTALL_DIR}/workdup"
else
  echo "==> ${INSTALL_DIR} is not writable, using sudo"
  need sudo
  sudo mv "${tmp}/workdup" "${INSTALL_DIR}/workdup"
fi

echo "==> installed ${INSTALL_DIR}/workdup"
"${INSTALL_DIR}/workdup" --help >/dev/null 2>&1 \
  && echo "==> ok" \
  || echo "==> warning: the binary did not run cleanly; check that ${INSTALL_DIR} is on your PATH"
