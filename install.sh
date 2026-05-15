#!/bin/sh
# install.sh — smart installer for the `rage` CLI.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/thehumanworks/rage/main/install.sh | sh
#
# Environment overrides:
#   RAGE_REPO          owner/repo to install from        (default: thehumanworks/rage)
#   VERSION            release tag to install            (default: latest)
#   PREFIX             install prefix override           (default: /usr/local or $HOME/.local)
#   INSTALL_DIR        explicit install bin dir          (default: $PREFIX/bin)
#   RAGE_NO_SUDO       if 1, never use sudo              (default: unset)
#   GITHUB_TOKEN       auth token for private repos      (default: unset)
#   RAGE_GITHUB_TOKEN  alias for GITHUB_TOKEN
#
# If RAGE_REPO is a private GitHub repo, GITHUB_TOKEN must be set to a PAT or
# fine-grained token with `contents:read` on the repo. The script will then
# fetch release metadata and asset bytes through the authenticated API so the
# private repo's binaries can be installed.
#
# The script:
#   1. detects host OS and CPU via uname,
#   2. resolves the matching release asset name,
#   3. downloads the archive + SHA256SUMS via the GitHub Releases API,
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

# ----- Auth ------------------------------------------------------------------
TOKEN="${RAGE_GITHUB_TOKEN:-${GITHUB_TOKEN:-}}"
if [ -z "$TOKEN" ] && command -v gh >/dev/null 2>&1; then
  # Fall back to the gh CLI's stored token if available (silent if not signed in).
  TOKEN="$(gh auth token 2>/dev/null || true)"
fi

# ----- Fetch helpers (curl preferred, wget fallback) -------------------------
have_curl=0
have_wget=0
command -v curl >/dev/null 2>&1 && have_curl=1
command -v wget >/dev/null 2>&1 && have_wget=1
if [ "$have_curl" -eq 0 ] && [ "$have_wget" -eq 0 ]; then
  die "neither curl nor wget is installed; install one and retry"
fi

# api_get URL → stdout (JSON). Sends Authorization if TOKEN is set, Accept JSON.
api_get() {
  url="$1"
  if [ "$have_curl" -eq 1 ]; then
    if [ -n "$TOKEN" ]; then
      curl -fsSL -H "Authorization: Bearer $TOKEN" -H 'Accept: application/vnd.github+json' "$url"
    else
      curl -fsSL -H 'Accept: application/vnd.github+json' "$url"
    fi
  else
    if [ -n "$TOKEN" ]; then
      wget -qO- --header="Authorization: Bearer $TOKEN" --header='Accept: application/vnd.github+json' "$url"
    else
      wget -qO- --header='Accept: application/vnd.github+json' "$url"
    fi
  fi
}

# asset_get URL OUT → file. Uses Accept: octet-stream so the API returns the
# binary, not a JSON descriptor.
asset_get() {
  url="$1"; out="$2"
  if [ "$have_curl" -eq 1 ]; then
    if [ -n "$TOKEN" ]; then
      curl -fsSL -H "Authorization: Bearer $TOKEN" -H 'Accept: application/octet-stream' "$url" -o "$out"
    else
      curl -fsSL -H 'Accept: application/octet-stream' "$url" -o "$out"
    fi
  else
    if [ -n "$TOKEN" ]; then
      wget -qO "$out" --header="Authorization: Bearer $TOKEN" --header='Accept: application/octet-stream' "$url"
    else
      wget -qO "$out" --header='Accept: application/octet-stream' "$url"
    fi
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

# Resolve release metadata (one API call gets the tag and all asset URLs).
if [ "$VERSION" = "latest" ]; then
  release_api="https://api.github.com/repos/$RAGE_REPO/releases/latest"
else
  release_api="https://api.github.com/repos/$RAGE_REPO/releases/tags/$VERSION"
fi

release_json="$(api_get "$release_api" 2>&1)" || {
  msg="$release_json"
  case "$msg" in
    *404*|*"Not Found"*)
      die "release lookup returned 404 for $release_api. If $RAGE_REPO is a private repo, export GITHUB_TOKEN (a fine-grained PAT with contents:read on the repo) and re-run."
      ;;
    *)
      die "release lookup failed: $msg"
      ;;
  esac
}

# Bootstrap: pull the tag from the JSON so we can derive the archive name.
tag="$(printf '%s' "$release_json" | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n1)"
[ -n "$tag" ] || die "could not parse tag_name from release JSON"
version="${tag#v}"
archive_name="rage-${version}-${target}.tar.gz"
sums_name="SHA256SUMS"

# Resolve asset API URLs from the JSON. Python3 is used because robust JSON
# parsing in pure sh is too fragile against GitHub's response shape.
tmpdir="$(mktemp -d 2>/dev/null || mktemp -d -t rage-install)"
trap 'rm -rf "$tmpdir"' EXIT INT TERM HUP
printf '%s' "$release_json" > "$tmpdir/release.json"

if command -v python3 >/dev/null 2>&1; then
  python_bin=python3
elif command -v python >/dev/null 2>&1; then
  python_bin=python
else
  die "python (2.7 or 3.x) is required to parse the GitHub release JSON"
fi

parsed="$("$python_bin" -c '
import json, sys
data = json.load(open(sys.argv[1]))
want = {"archive": sys.argv[2], "sums": sys.argv[3]}
assets = {a["name"]: a["url"] for a in data.get("assets", [])}
print(assets.get(want["archive"], ""))
print(assets.get(want["sums"], ""))
' "$tmpdir/release.json" "$archive_name" "$sums_name")"

archive_url="$(printf '%s\n' "$parsed" | sed -n '1p')"
sums_url="$(printf '%s\n' "$parsed" | sed -n '2p')"

[ -n "$archive_url" ] || die "archive $archive_name not found in release $tag assets"
[ -n "$sums_url" ] || die "$sums_name not found in release $tag assets"

log "target:   $target"
log "tag:      $tag"
log "archive:  $archive_name"
log "auth:     $([ -n "$TOKEN" ] && echo 'yes (token present)' || echo 'no (anonymous)')"

archive_path="$tmpdir/$archive_name"
sums_path="$tmpdir/$sums_name"

asset_get "$archive_url" "$archive_path" || die "download failed: $archive_url"
asset_get "$sums_url" "$sums_path" || die "download failed: $sums_url"

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
