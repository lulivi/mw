#!/usr/bin/env bash
# Downloads the latest mw release for this Linux or macOS machine into the current directory:
#   curl -fsSL https://raw.githubusercontent.com/lulivi/mw/main/scripts/install.sh | bash
set -euo pipefail

main() {
  local releases=https://github.com/lulivi/mw/releases
  local os arch
  case "$(uname -s)" in
    Linux) os=unknown-linux-musl ;;
    Darwin) os=apple-darwin ;;
    *) fail "unsupported system $(uname -s); see $releases" ;;
  esac
  case "$(uname -m)" in
    x86_64 | amd64) arch=x86_64 ;;
    aarch64 | arm64) arch=aarch64 ;;
    *) fail "unsupported CPU $(uname -m); see $releases" ;;
  esac

  local archive="mw-$arch-$os.tar.gz"
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  echo "Downloading $archive"
  curl -fsSL -o "$tmp/$archive" "$releases/latest/download/$archive"
  curl -fsSL -o "$tmp/$archive.sha256" "$releases/latest/download/$archive.sha256"

  if command -v sha256sum >/dev/null; then
    (cd "$tmp" && sha256sum -c "$archive.sha256" >/dev/null) || fail "checksum mismatch for $archive"
  else
    (cd "$tmp" && shasum -a 256 -c "$archive.sha256" >/dev/null) || fail "checksum mismatch for $archive"
  fi

  tar -xzf "$tmp/$archive" -C "$tmp" mw
  mv -f "$tmp/mw" ./mw
  echo "Installed $PWD/mw. Run ./mw --help to get started."
}

fail() {
  echo "error: $*" >&2
  exit 1
}

main "$@"
