#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  release-local.sh \
    --tag vVERSION \
    [--output-dir DIRECTORY] \
    [--base-url HTTPS_URL] \
    [--signing-source keychain|p12-env] \
    [--notary-auth keychain-profile|apple-id] \
    [--publish-r2] \
    [--reuse-artifacts] \
    [--no-open]

Builds the complete Developer ID-signed and Apple-notarized public release on
this Mac for Apple Silicon and Intel. R2 is changed only with --publish-r2.

Default local authentication:
  --signing-source keychain
  --notary-auth keychain-profile

Required public release environment:
  APPLE_SIGNING_IDENTITY
  APPLE_TEAM_ID
  APPLE_KEYCHAIN_PROFILE              (default notary mode)

Required only with --publish-r2:
  CLOUDFLARE_R2_ACCESS_KEY_ID
  CLOUDFLARE_R2_SECRET_ACCESS_KEY
  CLOUDFLARE_R2_BUCKET
  CLOUDFLARE_R2_ENDPOINT

CLOUDFLARE_R2_PUBLIC_BASE_URL defaults to:
  https://updates.key.duckgoo.net

Use --reuse-artifacts after an upload failure to retry the preserved bytes.
Rebuilding the same notarized version can produce different immutable bytes.
EOF
}

die() {
  printf 'release-local: %s\n' "$*" >&2
  exit 1
}

usage_error() {
  printf 'release-local: %s\n' "$*" >&2
  usage >&2
  exit 2
}

require_value() {
  local option="$1"
  local value="${2-}"
  [[ -n "$value" ]] || usage_error "$option requires a non-empty value"
}

tag=""
output_dir=""
base_url="${CLOUDFLARE_R2_PUBLIC_BASE_URL:-https://updates.key.duckgoo.net}"
signing_source="keychain"
notary_auth="keychain-profile"
publish_r2="false"
reuse_artifacts="false"
open_output="true"

while (( $# > 0 )); do
  case "$1" in
    --tag)
      require_value "$1" "${2-}"
      tag="$2"
      shift 2
      ;;
    --output-dir)
      require_value "$1" "${2-}"
      output_dir="$2"
      shift 2
      ;;
    --base-url)
      require_value "$1" "${2-}"
      base_url="$2"
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
    --publish-r2)
      publish_r2="true"
      shift
      ;;
    --reuse-artifacts)
      reuse_artifacts="true"
      shift
      ;;
    --no-open)
      open_output="false"
      shift
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

[[ -n "$tag" ]] || usage_error "--tag is required"
if [[ ! "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$ ]]; then
  usage_error "tag must be SemVer beginning with v"
fi
case "$signing_source" in
  keychain|p12-env)
    ;;
  *)
    usage_error "unsupported signing source: $signing_source"
    ;;
esac
case "$notary_auth" in
  keychain-profile|apple-id)
    ;;
  *)
    usage_error "unsupported notarization authentication: $notary_auth"
    ;;
esac
if [[ ! "$base_url" =~ ^https://[^/?#[:space:]]+([^?#[:space:]]*)?$ ]]; then
  usage_error "base-url must be an HTTPS origin or path without a query or fragment"
fi
while [[ "$base_url" == */ ]]; do
  base_url="${base_url%/}"
done

if [[ "$(uname -s)" != "Darwin" ]]; then
  die "local public release packaging must run on macOS"
fi
command -v git >/dev/null 2>&1 || die "git is required"
command -v mise >/dev/null 2>&1 \
  || die "mise is required; install it and run mise install --locked"

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_dir="$(cd "$script_dir/.." && pwd)"
cd "$project_dir"

if [[ -n "$(git status --porcelain --untracked-files=all)" ]]; then
  die "the working tree must be clean for a public release"
fi

tag_commit="$(git rev-parse -q --verify "refs/tags/$tag^{commit}" 2>/dev/null || true)"
[[ -n "$tag_commit" ]] || die "release tag does not exist locally: $tag"
head_commit="$(git rev-parse HEAD)"
[[ "$tag_commit" == "$head_commit" ]] \
  || die "release tag $tag does not point to HEAD"

printf 'Preparing pinned release tools with mise...\n'
mise install --locked rust cargo:cargo-packager python jq aws

version="$(
  MISE_AUTO_INSTALL=false MISE_EXEC_AUTO_INSTALL=false \
    mise exec -- python3 - "$project_dir/Cargo.toml" <<'PY'
import pathlib
import sys
import tomllib

cargo = tomllib.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
print(cargo["package"]["version"])
PY
)"
if [[ "$tag" != "v$version" ]]; then
  die "tag $tag does not match Cargo.toml package version $version"
fi

tag_date="$(git for-each-ref --format='%(taggerdate:iso-strict)' "refs/tags/$tag")"
if [[ -n "$tag_date" ]]; then
  release_date="$tag_date"
else
  release_date="$(git show -s --format=%cI "$head_commit")"
fi
pub_date="$(
  MISE_AUTO_INSTALL=false MISE_EXEC_AUTO_INSTALL=false \
    mise exec -- python3 - "$release_date" <<'PY'
from datetime import datetime, timezone
import sys

value = sys.argv[1]
if value.endswith("Z"):
    value = value[:-1] + "+00:00"
parsed = datetime.fromisoformat(value).astimezone(timezone.utc)
print(parsed.strftime("%Y-%m-%dT%H:%M:%SZ"))
PY
)"

if [[ -z "$output_dir" ]]; then
  output_dir="$project_dir/target/release/public/$tag"
elif [[ "$output_dir" != /* ]]; then
  output_dir="$project_dir/$output_dir"
fi

verify_remote_release_tag() {
  local remote_tags=""
  local remote_commit=""

  remote_tags="$(git ls-remote --tags origin "refs/tags/$tag" "refs/tags/$tag^{}")" \
    || die "could not read release tag $tag from origin"
  remote_commit="$(
    printf '%s\n' "$remote_tags" | awk '
      $2 ~ /\^\{\}$/ { peeled = $1 }
      $2 !~ /\^\{\}$/ { direct = $1 }
      END { if (peeled != "") print peeled; else print direct }
    '
  )"
  [[ "$remote_commit" == "$head_commit" ]] \
    || die "origin tag $tag does not resolve to the release commit"
}

if [[ "$publish_r2" == "true" ]]; then
  missing=()
  [[ -n "${CLOUDFLARE_R2_ACCESS_KEY_ID:-}" ]] || missing+=("CLOUDFLARE_R2_ACCESS_KEY_ID")
  [[ -n "${CLOUDFLARE_R2_SECRET_ACCESS_KEY:-}" ]] || missing+=("CLOUDFLARE_R2_SECRET_ACCESS_KEY")
  [[ -n "${CLOUDFLARE_R2_BUCKET:-}" ]] || missing+=("CLOUDFLARE_R2_BUCKET")
  [[ -n "${CLOUDFLARE_R2_ENDPOINT:-}" ]] || missing+=("CLOUDFLARE_R2_ENDPOINT")
  if (( ${#missing[@]} > 0 )); then
    printf -v missing_list '%s, ' "${missing[@]}"
    die "R2 publication is missing ${missing_list%, }"
  fi

  verify_remote_release_tag

  printf 'Checking R2 access before building release artifacts...\n'
  AWS_ACCESS_KEY_ID="$CLOUDFLARE_R2_ACCESS_KEY_ID" \
  AWS_SECRET_ACCESS_KEY="$CLOUDFLARE_R2_SECRET_ACCESS_KEY" \
  AWS_DEFAULT_REGION=auto \
  AWS_EC2_METADATA_DISABLED=true \
  MISE_AUTO_INSTALL=false MISE_EXEC_AUTO_INSTALL=false \
    mise exec -- aws \
      --endpoint-url "$CLOUDFLARE_R2_ENDPOINT" \
      --no-cli-pager \
      s3api list-objects-v2 \
      --bucket "$CLOUDFLARE_R2_BUCKET" \
      --max-keys 1 \
      --output json >/dev/null
fi

expected_files=(
  "DuckGooKey-$version-macos-aarch64.dmg"
  "DuckGooKey-$version-macos-aarch64.app.zip"
  "DuckGooKey-$version-macos-aarch64.SHA256SUMS"
  "DuckGooKey-$version-macos-x86_64.dmg"
  "DuckGooKey-$version-macos-x86_64.app.zip"
  "DuckGooKey-$version-macos-x86_64.SHA256SUMS"
)

verify_staged_files() {
  local name=""

  [[ -d "$output_dir" ]] || die "artifact directory does not exist: $output_dir"
  for name in "${expected_files[@]}"; do
    [[ -s "$output_dir/$name" ]] || die "missing or empty staged artifact: $output_dir/$name"
  done
  (
    cd "$output_dir"
    shasum -a 256 -c "DuckGooKey-$version-macos-aarch64.SHA256SUMS"
    shasum -a 256 -c "DuckGooKey-$version-macos-x86_64.SHA256SUMS"
  )
}

verify_release_receipt() {
  local receipt="$output_dir/public-release.json"

  [[ -s "$receipt" ]] || die "public release receipt is missing: $receipt"
  MISE_AUTO_INSTALL=false MISE_EXEC_AUTO_INSTALL=false \
    mise exec -- jq -e \
      --arg version "$version" \
      --arg tag "$tag" \
      --arg commit "$head_commit" \
      --arg pub_date "$pub_date" \
      --arg base_url "$base_url" \
      '
        .schema == 1
        and .distribution == "public-notarized"
        and .version == $version
        and .tag == $tag
        and .commit == $commit
        and .pub_date == $pub_date
        and .base_url == $base_url
      ' "$receipt" >/dev/null \
    || die "public release receipt does not match this release"
}

work_dir=""
cleanup() {
  if [[ -n "$work_dir" && -d "$work_dir" ]]; then
    rm -rf "$work_dir"
  fi
}
trap cleanup EXIT

if [[ "$reuse_artifacts" == "true" ]]; then
  printf 'Reusing preserved release artifacts from %s\n' "$output_dir"
  verify_release_receipt
  verify_staged_files
else
  if [[ -e "$output_dir" ]]; then
    if [[ -d "$output_dir" && -z "$(find "$output_dir" -mindepth 1 -print -quit)" ]]; then
      rmdir "$output_dir"
    else
      die "output path already exists; use a new path or --reuse-artifacts: $output_dir"
    fi
  fi

  output_parent="$(dirname "$output_dir")"
  mkdir -p "$output_parent"
  work_dir="$(mktemp -d "$output_parent/.duckgookey-public-$tag.XXXXXX")"

  MISE_AUTO_INSTALL=false MISE_EXEC_AUTO_INSTALL=false \
    mise exec -- "$script_dir/package-macos-release-arch.sh" \
      --distribution public-notarized \
      --version "$version" \
      --platform macos-aarch64 \
      --stage-dir "$work_dir" \
      --signing-source "$signing_source" \
      --notary-auth "$notary_auth"

  MISE_AUTO_INSTALL=false MISE_EXEC_AUTO_INSTALL=false \
    mise exec -- "$script_dir/package-macos-release-arch.sh" \
      --distribution public-notarized \
      --version "$version" \
      --platform macos-x86_64 \
      --stage-dir "$work_dir" \
      --signing-source "$signing_source" \
      --notary-auth "$notary_auth"

  MISE_AUTO_INSTALL=false MISE_EXEC_AUTO_INSTALL=false \
    mise exec -- "$script_dir/build-release-manifest.sh" \
      --version "$version" \
      --pub-date "$pub_date" \
      --base-url "$base_url" \
      --artifacts-dir "$work_dir" \
      --output "$work_dir/latest.json"

  MISE_AUTO_INSTALL=false MISE_EXEC_AUTO_INSTALL=false \
    mise exec -- jq -n \
      --arg version "$version" \
      --arg tag "$tag" \
      --arg commit "$head_commit" \
      --arg pub_date "$pub_date" \
      --arg base_url "$base_url" \
      '{
        schema: 1,
        distribution: "public-notarized",
        version: $version,
        tag: $tag,
        commit: $commit,
        pub_date: $pub_date,
        base_url: $base_url
      }' > "$work_dir/public-release.json"

  mv "$work_dir" "$output_dir"
  work_dir=""
  verify_staged_files
fi

# Rebuild the deterministic manifest when reusing preserved binaries. This
# also rejects a changed base URL before publication.
if [[ "$reuse_artifacts" == "true" ]]; then
  MISE_AUTO_INSTALL=false MISE_EXEC_AUTO_INSTALL=false \
    mise exec -- "$script_dir/build-release-manifest.sh" \
      --version "$version" \
      --pub-date "$pub_date" \
      --base-url "$base_url" \
      --artifacts-dir "$output_dir" \
      --output "$output_dir/latest.json"
fi

if [[ "$publish_r2" == "true" ]]; then
  if [[ -n "$(git status --porcelain --untracked-files=all)" ]]; then
    die "the working tree changed while release artifacts were being built"
  fi
  [[ "$(git rev-parse HEAD)" == "$head_commit" ]] \
    || die "HEAD changed while release artifacts were being built"
  verify_remote_release_tag

  printf 'Publishing DuckGooKey %s to R2...\n' "$tag"
  AWS_ACCESS_KEY_ID="$CLOUDFLARE_R2_ACCESS_KEY_ID" \
  AWS_SECRET_ACCESS_KEY="$CLOUDFLARE_R2_SECRET_ACCESS_KEY" \
  AWS_DEFAULT_REGION=auto \
  AWS_EC2_METADATA_DISABLED=true \
  MISE_AUTO_INSTALL=false MISE_EXEC_AUTO_INSTALL=false \
    mise exec -- "$script_dir/publish-r2.sh" \
      --version "$version" \
      --bucket "$CLOUDFLARE_R2_BUCKET" \
      --endpoint "$CLOUDFLARE_R2_ENDPOINT" \
      --base-url "$base_url" \
      --artifacts-dir "$output_dir" \
      --manifest "$output_dir/latest.json"
fi

printf '\nPublic release is ready:\n'
printf '  Tag: %s\n' "$tag"
printf '  Commit: %s\n' "$head_commit"
printf '  Artifacts: %s\n' "$output_dir"
printf '  R2 publication: %s\n' "$publish_r2"
if [[ "$publish_r2" == "true" ]]; then
  printf '  Manifest: %s/latest.json\n' "$base_url"
fi

if [[ "$open_output" == "true" ]]; then
  if ! open "$output_dir"; then
    printf 'release-local: release succeeded, but Finder could not open %s\n' "$output_dir" >&2
  fi
fi
