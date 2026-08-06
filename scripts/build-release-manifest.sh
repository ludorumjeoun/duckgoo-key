#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  build-release-manifest.sh \
    --version VERSION \
    --pub-date RFC3339_UTC \
    --base-url HTTPS_URL \
    --artifacts-dir DIRECTORY \
    --output FILE

Builds the DuckGooKey latest.json manifest for the two macOS DMG installers and
app bundle ZIP files in DIRECTORY. Artifact URLs use the immutable
releases/vVERSION/ object prefix.
EOF
}

die() {
  printf 'build-release-manifest: %s\n' "$*" >&2
  exit 1
}

require_value() {
  local option="$1"
  local value="${2-}"
  [[ -n "$value" ]] || die "$option requires a non-empty value"
}

version=""
pub_date=""
base_url=""
artifacts_dir=""
output=""

while (( $# > 0 )); do
  case "$1" in
    --version)
      require_value "$1" "${2-}"
      version="$2"
      shift 2
      ;;
    --pub-date)
      require_value "$1" "${2-}"
      pub_date="$2"
      shift 2
      ;;
    --base-url)
      require_value "$1" "${2-}"
      base_url="$2"
      shift 2
      ;;
    --artifacts-dir)
      require_value "$1" "${2-}"
      artifacts_dir="$2"
      shift 2
      ;;
    --output)
      require_value "$1" "${2-}"
      output="$2"
      shift 2
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
done

[[ -n "$version" ]] || die "--version is required"
[[ -n "$pub_date" ]] || die "--pub-date is required"
[[ -n "$base_url" ]] || die "--base-url is required"
[[ -n "$artifacts_dir" ]] || die "--artifacts-dir is required"
[[ -n "$output" ]] || die "--output is required"

if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$ ]]; then
  die "version must be SemVer without a leading v"
fi
if [[ ! "$pub_date" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$ ]]; then
  die "pub-date must be UTC RFC3339 in YYYY-MM-DDTHH:MM:SSZ form"
fi
if [[ ! "$base_url" =~ ^https://[^/?#[:space:]]+([^?#[:space:]]*)?$ ]]; then
  die "base-url must be an HTTPS origin or path without a query or fragment"
fi
[[ -d "$artifacts_dir" ]] || die "artifacts directory does not exist: $artifacts_dir"

command -v jq >/dev/null 2>&1 || die "jq is required"

while [[ "$base_url" == */ ]]; do
  base_url="${base_url%/}"
done

sha256_file() {
  local file="$1"
  local digest=""

  if command -v shasum >/dev/null 2>&1; then
    digest="$(shasum -a 256 "$file" | awk '{print $1}')"
  elif command -v sha256sum >/dev/null 2>&1; then
    digest="$(sha256sum "$file" | awk '{print $1}')"
  else
    die "shasum or sha256sum is required"
  fi

  [[ "$digest" =~ ^[0-9a-f]{64}$ ]] || die "failed to calculate SHA-256 for: $file"
  printf '%s\n' "$digest"
}

aarch64_name="DuckGooKey-$version-macos-aarch64.dmg"
x86_64_name="DuckGooKey-$version-macos-x86_64.dmg"
aarch64_app_name="DuckGooKey-$version-macos-aarch64.app.zip"
x86_64_app_name="DuckGooKey-$version-macos-x86_64.app.zip"
aarch64_path="$artifacts_dir/$aarch64_name"
x86_64_path="$artifacts_dir/$x86_64_name"
aarch64_app_path="$artifacts_dir/$aarch64_app_name"
x86_64_app_path="$artifacts_dir/$x86_64_app_name"

[[ -s "$aarch64_path" ]] || die "missing or empty release artifact: $aarch64_path"
[[ -s "$x86_64_path" ]] || die "missing or empty release artifact: $x86_64_path"
[[ -s "$aarch64_app_path" ]] || die "missing or empty release artifact: $aarch64_app_path"
[[ -s "$x86_64_app_path" ]] || die "missing or empty release artifact: $x86_64_app_path"

aarch64_sha256="$(sha256_file "$aarch64_path")"
x86_64_sha256="$(sha256_file "$x86_64_path")"
aarch64_app_sha256="$(sha256_file "$aarch64_app_path")"
x86_64_app_sha256="$(sha256_file "$x86_64_app_path")"
aarch64_url="$base_url/releases/v$version/$aarch64_name"
x86_64_url="$base_url/releases/v$version/$x86_64_name"
aarch64_app_url="$base_url/releases/v$version/$aarch64_app_name"
x86_64_app_url="$base_url/releases/v$version/$x86_64_app_name"

output_dir="$(dirname "$output")"
mkdir -p "$output_dir"
tmp_output="$(mktemp "$output.tmp.XXXXXX")"
cleanup() {
  rm -f "$tmp_output"
}
trap cleanup EXIT

jq -n \
  --arg version "$version" \
  --arg pub_date "$pub_date" \
  --arg aarch64_url "$aarch64_url" \
  --arg aarch64_sha256 "$aarch64_sha256" \
  --arg aarch64_app_url "$aarch64_app_url" \
  --arg aarch64_app_sha256 "$aarch64_app_sha256" \
  --arg x86_64_url "$x86_64_url" \
  --arg x86_64_sha256 "$x86_64_sha256" \
  --arg x86_64_app_url "$x86_64_app_url" \
  --arg x86_64_app_sha256 "$x86_64_app_sha256" \
  '{
    version: $version,
    pub_date: $pub_date,
    platforms: {
      "macos-aarch64": {
        url: $aarch64_url,
        sha256: $aarch64_sha256,
        app_url: $aarch64_app_url,
        app_sha256: $aarch64_app_sha256
      },
      "macos-x86_64": {
        url: $x86_64_url,
        sha256: $x86_64_sha256,
        app_url: $x86_64_app_url,
        app_sha256: $x86_64_app_sha256
      }
    }
  }' > "$tmp_output"

jq -e \
  '(keys | sort == ["platforms", "pub_date", "version"])
   and (.platforms | keys | sort == ["macos-aarch64", "macos-x86_64"])
   and (.platforms["macos-aarch64"] | keys | sort == ["app_sha256", "app_url", "sha256", "url"])
   and (.platforms["macos-x86_64"] | keys | sort == ["app_sha256", "app_url", "sha256", "url"])
   and (.platforms["macos-aarch64"].sha256 | test("^[0-9a-f]{64}$"))
   and (.platforms["macos-x86_64"].sha256 | test("^[0-9a-f]{64}$"))
   and (.platforms["macos-aarch64"].app_sha256 | test("^[0-9a-f]{64}$"))
   and (.platforms["macos-x86_64"].app_sha256 | test("^[0-9a-f]{64}$"))' \
  "$tmp_output" >/dev/null

mv "$tmp_output" "$output"
trap - EXIT
printf 'Wrote release manifest: %s\n' "$output"
