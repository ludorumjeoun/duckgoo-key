#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  package-macos-release-arch.sh \
    --distribution public-notarized|unsigned-diagnostic \
    --version VERSION \
    --platform macos-aarch64|macos-x86_64 \
    --stage-dir DIRECTORY \
    [--signing-source keychain|p12-env] \
    [--notary-auth keychain-profile|apple-id]

Builds and stages one architecture using the same artifact contract for local
and GitHub releases.

Public release environment:
  APPLE_SIGNING_IDENTITY   Developer ID Application identity
  APPLE_TEAM_ID            Apple Developer Team ID

With --signing-source keychain, the identity must already be installed.
With --signing-source p12-env, also provide:
  APPLE_CERTIFICATE
  APPLE_CERTIFICATE_PASSWORD

With --notary-auth keychain-profile, also provide:
  APPLE_KEYCHAIN_PROFILE

With --notary-auth apple-id, also provide:
  APPLE_ID
  APPLE_PASSWORD

Unsigned diagnostic mode strips every Apple credential from the packaging
process and must never be published as a public release.
EOF
}

die() {
  printf 'package-macos-release-arch: %s\n' "$*" >&2
  exit 1
}

usage_error() {
  printf 'package-macos-release-arch: %s\n' "$*" >&2
  usage >&2
  exit 2
}

require_value() {
  local option="$1"
  local value="${2-}"
  [[ -n "$value" ]] || usage_error "$option requires a non-empty value"
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "required command is unavailable: $1"
}

distribution=""
version=""
platform=""
stage_dir=""
signing_source=""
notary_auth=""

while (( $# > 0 )); do
  case "$1" in
    --distribution)
      require_value "$1" "${2-}"
      distribution="$2"
      shift 2
      ;;
    --version)
      require_value "$1" "${2-}"
      version="$2"
      shift 2
      ;;
    --platform)
      require_value "$1" "${2-}"
      platform="$2"
      shift 2
      ;;
    --stage-dir)
      require_value "$1" "${2-}"
      stage_dir="$2"
      shift 2
      ;;
    --signing-source)
      require_value "$1" "${2-}"
      signing_source="$2"
      shift 2
      ;;
    --notary-auth)
      require_value "$1" "${2-}"
      notary_auth="$2"
      shift 2
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      usage_error "unknown argument: $1"
      ;;
  esac
done

[[ -n "$distribution" ]] || usage_error "--distribution is required"
[[ -n "$version" ]] || usage_error "--version is required"
[[ -n "$platform" ]] || usage_error "--platform is required"
[[ -n "$stage_dir" ]] || usage_error "--stage-dir is required"

case "$distribution" in
  public-notarized|unsigned-diagnostic)
    ;;
  *)
    usage_error "unsupported distribution: $distribution"
    ;;
esac

if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$ ]]; then
  usage_error "version must be SemVer without a leading v"
fi

case "$platform" in
  macos-aarch64)
    target="aarch64-apple-darwin"
    expected_arch="arm64"
    ;;
  macos-x86_64)
    target="x86_64-apple-darwin"
    expected_arch="x86_64"
    ;;
  *)
    usage_error "unsupported platform: $platform"
    ;;
esac

if [[ "$(uname -s)" != "Darwin" ]]; then
  die "macOS release packaging must run on macOS"
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_dir="$(cd "$script_dir/.." && pwd)"

require_command cargo
require_command cargo-packager
require_command codesign
require_command ditto
require_command grep
require_command hdiutil
require_command jq
require_command lipo
require_command security
require_command shasum
require_command spctl
require_command xcrun

notary_args=()
signing_enabled="false"
notarization_enabled="false"

if [[ "$distribution" == "public-notarized" ]]; then
  signing_enabled="true"
  notarization_enabled="true"

  [[ -n "${APPLE_SIGNING_IDENTITY:-}" ]] || die "APPLE_SIGNING_IDENTITY is required"
  [[ "$APPLE_SIGNING_IDENTITY" == "Developer ID Application:"* ]] \
    || die "APPLE_SIGNING_IDENTITY must be a Developer ID Application identity"
  [[ -n "${APPLE_TEAM_ID:-}" ]] || die "APPLE_TEAM_ID is required"
  [[ "$APPLE_TEAM_ID" =~ ^[A-Z0-9]{10}$ ]] || die "APPLE_TEAM_ID must contain 10 uppercase letters or digits"

  case "$signing_source" in
    keychain)
      unset APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD
      signing_identities="$(security find-identity -v -p codesigning)"
      if ! grep -Fq "\"$APPLE_SIGNING_IDENTITY\"" <<< "$signing_identities"; then
        die "the configured Developer ID identity is not installed in the local keychain"
      fi
      ;;
    p12-env)
      [[ -n "${APPLE_CERTIFICATE:-}" ]] || die "APPLE_CERTIFICATE is required for p12-env signing"
      [[ -n "${APPLE_CERTIFICATE_PASSWORD:-}" ]] \
        || die "APPLE_CERTIFICATE_PASSWORD is required for p12-env signing"
      ;;
    "")
      usage_error "public-notarized requires --signing-source"
      ;;
    *)
      usage_error "unsupported signing source: $signing_source"
      ;;
  esac

  case "$notary_auth" in
    keychain-profile)
      [[ -n "${APPLE_KEYCHAIN_PROFILE:-}" ]] \
        || die "APPLE_KEYCHAIN_PROFILE is required for keychain-profile notarization"
      unset \
        APPLE_API_ISSUER \
        APPLE_API_KEY \
        APPLE_API_KEY_PATH \
        APPLE_ID \
        APPLE_PASSWORD
      notary_args=(--keychain-profile "$APPLE_KEYCHAIN_PROFILE")
      ;;
    apple-id)
      [[ -n "${APPLE_ID:-}" ]] || die "APPLE_ID is required for Apple ID notarization"
      [[ -n "${APPLE_PASSWORD:-}" ]] || die "APPLE_PASSWORD is required for Apple ID notarization"
      unset \
        APPLE_API_ISSUER \
        APPLE_API_KEY \
        APPLE_API_KEY_PATH \
        APPLE_KEYCHAIN_PROFILE
      notary_args=(
        --apple-id "$APPLE_ID"
        --password "$APPLE_PASSWORD"
        --team-id "$APPLE_TEAM_ID"
      )
      ;;
    "")
      usage_error "public-notarized requires --notary-auth"
      ;;
    *)
      usage_error "unsupported notarization authentication: $notary_auth"
      ;;
  esac
else
  if [[ -n "$signing_source" || -n "$notary_auth" ]]; then
    usage_error "unsigned-diagnostic does not accept signing or notarization options"
  fi
  unset \
    APPLE_API_ISSUER \
    APPLE_API_KEY \
    APPLE_API_KEY_PATH \
    APPLE_CERTIFICATE \
    APPLE_CERTIFICATE_PASSWORD \
    APPLE_ID \
    APPLE_KEYCHAIN_PROFILE \
    APPLE_PASSWORD \
    APPLE_SIGNING_IDENTITY \
    APPLE_TEAM_ID
fi

mkdir -p "$stage_dir"

dmg_name="DuckGooKey-$version-$platform.dmg"
app_zip_name="DuckGooKey-$version-$platform.app.zip"
checksums_name="DuckGooKey-$version-$platform.SHA256SUMS"

for name in "$dmg_name" "$app_zip_name" "$checksums_name"; do
  [[ ! -e "$stage_dir/$name" ]] \
    || die "refusing to overwrite an existing staged artifact: $stage_dir/$name"
done

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/duckgookey-packager.XXXXXX")"
cleanup() {
  rm -rf "$work_dir"
}
trap cleanup EXIT

package_dir="$work_dir/packages"
config_path="$work_dir/packager.json"
mkdir -p "$package_dir"

if [[ "$signing_enabled" == "true" ]]; then
  macos_config="$(
    jq -n \
      --arg minimum_system_version "${MACOSX_DEPLOYMENT_TARGET:-13.0}" \
      --arg signing_identity "$APPLE_SIGNING_IDENTITY" \
      '{
        minimumSystemVersion: $minimum_system_version,
        backgroundApp: true,
        signingIdentity: $signing_identity
      }'
  )"
else
  macos_config="$(
    jq -n \
      --arg minimum_system_version "${MACOSX_DEPLOYMENT_TARGET:-13.0}" \
      '{
        minimumSystemVersion: $minimum_system_version,
        backgroundApp: true
      }'
  )"
fi

jq -n \
  --arg version "$version" \
  --arg out_dir "$package_dir" \
  --arg binaries_dir "$project_dir/target/$target/release" \
  --arg icon "$project_dir/assets/icons/DuckGooKey.icns" \
  --argjson macos "$macos_config" \
  '{
    name: "duckgoo-key",
    productName: "DuckGooKey",
    identifier: "com.duckgoo.key",
    version: $version,
    description: "A fast, keyboard-first desktop launcher written in Rust",
    category: "Utility",
    outDir: $out_dir,
    binariesDir: $binaries_dir,
    binaries: [{path: "DuckGooKey", main: true}],
    formats: ["app", "dmg"],
    icons: [$icon],
    macos: $macos
  }' > "$config_path"

cd "$project_dir"
printf 'Building DuckGooKey %s (%s)...\n' "$platform" "$target"
cargo build --release --locked --target "$target"

printf 'Packaging DuckGooKey %s as %s...\n' "$platform" "$distribution"
cargo-packager \
  --config "$config_path" \
  --target "$target" \
  --formats app,dmg

app_path="$package_dir/DuckGooKey.app"
executable_path="$app_path/Contents/MacOS/DuckGooKey"
[[ -x "$executable_path" ]] || die "packaged executable is missing: $executable_path"

actual_arch="$(lipo -archs "$executable_path")"
if [[ "$actual_arch" != "$expected_arch" ]]; then
  die "expected $expected_arch, but packaged executable contains: $actual_arch"
fi

shopt -s nullglob
dmg_candidates=("$package_dir"/*.dmg)
if (( ${#dmg_candidates[@]} != 1 )); then
  die "expected exactly one DMG in $package_dir; found ${#dmg_candidates[@]}"
fi
dmg_path="${dmg_candidates[0]}"

notary_log() {
  local submission_id="$1"
  local log_path="$work_dir/notary-log.json"

  [[ -n "$submission_id" ]] || return 0
  xcrun notarytool log \
    "$submission_id" \
    "${notary_args[@]}" \
    "$log_path" >/dev/null 2>&1 || true
  [[ ! -s "$log_path" ]] || cat "$log_path" >&2
}

if [[ "$notarization_enabled" == "true" ]]; then
  result_path="$work_dir/notary-result.json"
  printf 'Submitting the final %s DMG for notarization...\n' "$platform"
  if ! xcrun notarytool submit \
    "$dmg_path" \
    "${notary_args[@]}" \
    --wait \
    --output-format json > "$result_path"; then
    submission_id="$(jq -r '.id // empty' "$result_path" 2>/dev/null || true)"
    notary_log "$submission_id"
    [[ ! -s "$result_path" ]] || cat "$result_path" >&2
    die "Apple rejected or could not process the final DMG"
  fi

  submission_id="$(jq -r '.id // empty' "$result_path")"
  status="$(jq -r '.status // empty' "$result_path")"
  if [[ "$status" != "Accepted" ]]; then
    notary_log "$submission_id"
    cat "$result_path" >&2
    die "final DMG notarization status is '$status', not 'Accepted'"
  fi

  xcrun stapler staple "$dmg_path"
  xcrun stapler validate "$dmg_path"
fi

verify_developer_id_signature() {
  local path="$1"
  local label="$2"
  local require_runtime="$3"
  local details=""

  codesign --verify --deep --strict --verbose=2 "$path"
  details="$(codesign --display --verbose=4 "$path" 2>&1)"

  grep -Fq "Authority=$APPLE_SIGNING_IDENTITY" <<< "$details" \
    || die "$label was not signed by the configured Developer ID identity"
  grep -Fq "TeamIdentifier=$APPLE_TEAM_ID" <<< "$details" \
    || die "$label TeamIdentifier does not match APPLE_TEAM_ID"
  grep -Fq 'Timestamp=' <<< "$details" \
    || die "$label does not contain a secure signing timestamp"
  if [[ "$require_runtime" == "true" ]]; then
    grep -Eq '^CodeDirectory .*flags=.*\(runtime\)' <<< "$details" \
      || die "$label does not have hardened runtime enabled"
  fi
}

if [[ "$signing_enabled" == "true" ]]; then
  verify_developer_id_signature "$app_path" "packaged app" "true"
  verify_developer_id_signature "$dmg_path" "final DMG" "false"
fi

if [[ "$notarization_enabled" == "true" ]]; then
  xcrun stapler validate "$app_path"
  xcrun stapler validate "$dmg_path"
  spctl --assess --type execute --verbose=4 "$app_path"
  spctl --assess \
    --type open \
    --context context:primary-signature \
    --verbose=4 \
    "$dmg_path"
fi

hdiutil verify "$dmg_path"

artifact_dir="$work_dir/artifacts"
mkdir -p "$artifact_dir"
mv "$dmg_path" "$artifact_dir/$dmg_name"
ditto -c -k --keepParent --sequesterRsrc \
  "$app_path" \
  "$artifact_dir/$app_zip_name"

(
  cd "$artifact_dir"
  shasum -a 256 "$dmg_name" "$app_zip_name" > "$checksums_name"
)

for name in "$dmg_name" "$app_zip_name" "$checksums_name"; do
  mv "$artifact_dir/$name" "$stage_dir/$name"
done

printf 'Staged %s artifacts in %s\n' "$distribution" "$stage_dir"
