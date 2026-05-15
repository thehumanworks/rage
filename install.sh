#!/bin/sh
# install.sh — smart installer for the `rage` CLI.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/thehumanworks/rage/main/install.sh | sh
#
# Environment overrides:
#   RAGE_REPO      owner/repo to install from   (default: thehumanworks/rage)
#   VERSION        release tag to install       (default: latest)
#   PREFIX         install prefix override      (default: /usr/local or $HOME/.local)
#   INSTALL_DIR    explicit install bin dir     (default: $PREFIX/bin)
#   RAGE_NO_SUDO   if set to 1, never use sudo  (default: unset)
#
# The script:
#   1. detects host OS and CPU via uname,
#   2. resolves the matching release asset name,
#   3. downloads the archive + SHA256SUMS from the GitHub release,
#   4. verifies the archive checksum,
#   5. installs the `rage` binary to a system-wide location when possible,
#      otherwise to $HOME/.local/bin (and reminds you to PATH it).

set -eu

RAGE_REPO="${RAGE_REPO:-thehumanworks/rage}"
VERSION="${VERSION:-latest}"

log() { printf '%s\n' "rage-install: $*" >&2; }
die() { log "ERROR: $*"; exit 1; }

need() {
  command -v "$1" >/dev/null 2>&1 || die "required tool not found: $1"
}

# ----- Fetch helpers (curl preferred, wget fallback) -------------------------
have_curl=0
have_wget=0
command -v curl >/dev/null 2>&1 && have_curl=1
command -v wget >/dev/null 2>&1 && have_wget=1
if [ "$have_curl" -eq 0 ] && [ "$have_wget" -eq 0 ]; then
  die "neither curl nor wget is installed; install one and retry"
fi

http_get() {
  # http_get URL OUT
  url="$1"; out="$2"
  if [ "$have_curl" -eq 1 ]; then
    curl -fsSL "$url" -o "$out"
  else
    wget -qO "$out" "$url"
  fi
}

http_get_stdout() {
  url="$1"
  if [ "$have_curl" -eq 1 ]; then
    curl -fsSL "$url"
  else
    wget -qO- "$url"
  fi
}

# ----- Detect platform -------------------------------------------------------
detect_target() {
  uname_s="$(uname -s)"
  uname_m="$(uname -m)"

  case "$uname_s" in
    Darwin)
      case "$uname_m" in
        arm64|aarch64) printf 'aarch64-apple-darwin\n' ;;
        x86_64)
          die "macOS Intel (x86_64) is not currently in the release matrix; build from source with 'cargo install --path .'"
          ;;
        *)
          die "unsupported macOS architecture: $uname_m"
          ;;
      esac
      ;;
    Linux)
      case "$uname_m" in
        x86_64|amd64) printf 'x86_64-unknown-linux-gnu\n' ;;
        aarch64|arm64) printf 'aarch64-unknown-linux-gnu\n' ;;
        *)
          die "unsupported Linux architecture: $uname_m"
          ;;
      esac
      ;;
    MINGW*|MSYS*|CYGWIN*)
      die "Windows is supported by release artifacts but not by install.sh; download rage-<version>-x86_64-pc-windows-msvc.zip from https://github.com/$RAGE_REPO/releases and add it to PATH"
      ;;
    *)
      die "unsupported OS: $uname_s"
      ;;
  esac
}

# ----- Resolve version -------------------------------------------------------
resolve_version() {
  if [ "$VERSION" = "latest" ]; then
    api="https://api.github.com/repos/$RAGE_REPO/releases/latest"
    if [ "$have_curl" -eq 1 ]; then
      raw="$(curl -fsSL -H 'Accept: application/vnd.github+json' "$api" || true)"
    else
      raw="$(wget -qO- --header='Accept: application/vnd.github+json' "$api" || true)"
    fi
    [ -n "$raw" ] || die "could not query latest release from $api"
    # Extract tag_name without jq.
    tag="$(printf '%s' "$raw" | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n1)"
    [ -n "$tag" ] || die "could not parse tag_name from GitHub API response"
    printf '%s\n' "$tag"
  else
    printf '%s\n' "$VERSION"
  fi
}

# ----- Choose install dir ----------------------------------------------------
choose_install_dir() {
  if [ -n "${INSTALL_DIR:-}" ]; then
    printf '%s\n' "$INSTALL_DIR"
    return
  fi
  if [ -n "${PREFIX:-}" ]; then
    printf '%s/bin\n' "$PREFIX"
    return
  fi
  # Prefer /usr/local/bin if writable or if sudo is available.
  if [ -w /usr/local/bin ]; then
    printf '/usr/local/bin\n'
  elif [ "${RAGE_NO_SUDO:-0}" != "1" ] && command -v sudo >/dev/null 2>&1; then
    printf '/usr/local/bin\n'
  else
    printf '%s/.local/bin\n' "$HOME"
  fi
}

install_file() {
  src="$1"; dst="$2"
  dst_dir="$(dirname "$dst")"
  if [ ! -d "$dst_dir" ]; then
    if [ -w "$(dirname "$dst_dir")" ] || [ "$dst_dir" = "$HOME/.local/bin" ]; then
      mkdir -p "$dst_dir"
    elif [ "${RAGE_NO_SUDO:-0}" != "1" ] && command -v sudo >/dev/null 2>&1; then
      sudo mkdir -p "$dst_dir"
    else
      die "install dir $dst_dir does not exist and cannot be created without sudo"
    fi
  fi
  if [ -w "$dst_dir" ]; then
    install -m 0755 "$src" "$dst"
  elif [ "${RAGE_NO_SUDO:-0}" != "1" ] && command -v sudo >/dev/null 2>&1; then
    log "installing to $dst (needs sudo)"
    sudo install -m 0755 "$src" "$dst"
  else
    die "cannot write to $dst_dir; set INSTALL_DIR or PREFIX to a writable location"
  fi
}

# ----- Main ------------------------------------------------------------------
need uname
need tar
need mkdir

target="$(detect_target)"
tag="$(resolve_version)"
version="${tag#v}"

archive_name="rage-${version}-${target}.tar.gz"
sums_name="SHA256SUMS"
base_url="https://github.com/$RAGE_REPO/releases/download/$tag"
archive_url="$base_url/$archive_name"
sums_url="$base_url/$sums_name"

log "target:   $target"
log "tag:      $tag"
log "archive:  $archive_url"

tmpdir="$(mktemp -d 2>/dev/null || mktemp -d -t rage-install)"
trap 'rm -rf "$tmpdir"' EXIT INT TERM HUP

archive_path="$tmpdir/$archive_name"
sums_path="$tmpdir/$sums_name"

http_get "$archive_url" "$archive_path" || die "download failed: $archive_url"
http_get "$sums_url" "$sums_path" || die "download failed: $sums_url (expected SHA256SUMS in release)"

# ----- Verify checksum -------------------------------------------------------
expected="$(awk -v f="$archive_name" '$2 == f || $2 == "*" f { print $1 }' "$sums_path" | head -n1)"
[ -n "$expected" ] || die "checksum for $archive_name not found in $sums_name"

if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "$archive_path" | awk '{print $1}')"
elif command -v shasum >/dev/null 2>&1; then
  actual="$(shasum -a 256 "$archive_path" | awk '{print $1}')"
else
  die "no sha256 tool found (need sha256sum or shasum)"
fi

if [ "$expected" != "$actual" ]; then
  die "checksum mismatch for $archive_name (expected $expected, got $actual)"
fi
log "checksum: ok"

# ----- Extract ---------------------------------------------------------------
tar -C "$tmpdir" -xzf "$archive_path"
extracted_bin="$tmpdir/rage-${version}-${target}/rage"
[ -f "$extracted_bin" ] || die "archive did not contain rage binary at expected path"

# ----- Install ---------------------------------------------------------------
install_dir="$(choose_install_dir)"
target_path="$install_dir/rage"
install_file "$extracted_bin" "$target_path"

log "installed: $target_path"
case ":$PATH:" in
  *":$install_dir:"*) ;;
  *) log "note: $install_dir is not in your PATH; add it to use 'rage' directly" ;;
esac

# Print version (use the installed binary).
if [ -x "$target_path" ]; then
  "$target_path" --version 2>/dev/null || true
fi
