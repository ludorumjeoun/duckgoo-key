#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: ./scripts/build-local.sh [--no-open]

Builds DuckGooKey in release mode, creates a macOS .app and .dmg, and opens
the package output directory in Finder.

Options:
  --no-open  Build packages without opening Finder.
  -h, --help Show this help.
EOF
}

open_output="true"
while (( $# > 0 )); do
  case "$1" in
    --no-open)
      open_output="false"
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      printf 'build-local: unknown argument: %s\n' "$1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ "$(uname -s)" != "Darwin" ]]; then
  printf 'build-local: local app packaging is supported only on macOS.\n' >&2
  exit 1
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_dir="$(cd "$script_dir/.." && pwd)"
output_dir="$project_dir/target/release/packages"

if ! command -v mise >/dev/null 2>&1; then
  printf 'build-local: mise is required. Install it, then run mise install --locked.\n' >&2
  exit 1
fi

cd "$project_dir"

printf 'Preparing pinned Rust and packaging tools with mise...\n'
mise install --locked rust cargo:cargo-packager

printf 'Building DuckGooKey.app and DMG...\n'
CI=true MISE_AUTO_INSTALL=false MISE_EXEC_AUTO_INSTALL=false \
  mise exec -- cargo-packager \
  --release \
  --manifest-path "$project_dir/Cargo.toml" \
  --formats app,dmg

app_path="$output_dir/DuckGooKey.app"
if [[ ! -x "$app_path/Contents/MacOS/DuckGooKey" ]]; then
  printf 'build-local: packaged app is missing: %s\n' "$app_path" >&2
  exit 1
fi

dmg_path=""
for candidate in "$output_dir"/*.dmg; do
  if [[ -f "$candidate" && "${candidate##*/}" != rw.* ]]; then
    if [[ -z "$dmg_path" || "$candidate" -nt "$dmg_path" ]]; then
      dmg_path="$candidate"
    fi
  fi
done
if [[ -z "$dmg_path" ]]; then
  printf 'build-local: no DMG was created in %s\n' "$output_dir" >&2
  exit 1
fi
if ! hdiutil verify "$dmg_path" >/dev/null; then
  printf 'build-local: DMG verification failed: %s\n' "$dmg_path" >&2
  exit 1
fi

printf '\nLocal packages are ready:\n'
printf '  App: %s\n' "$app_path"
printf '  DMG: %s\n' "$dmg_path"
printf '  Folder: %s\n' "$output_dir"
printf 'Open the DMG and drag DuckGooKey to Applications to install it.\n'

if [[ "$open_output" == "true" ]]; then
  open "$output_dir"
fi
