#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  publish-r2.sh \
    --version VERSION \
    --bucket BUCKET \
    --endpoint HTTPS_R2_ENDPOINT \
    --base-url HTTPS_PUBLIC_URL \
    --artifacts-dir DIRECTORY \
    --manifest FILE

Required environment:
  AWS_ACCESS_KEY_ID
  AWS_SECRET_ACCESS_KEY

Versioned objects are written beneath releases/vVERSION/ and are immutable.
An existing object is accepted only when its stored SHA-256 metadata matches.
The public custom domain is checked before latest.json advertises the release.
latest.json uses a conditional update and can never move to an older SemVer.
EOF
}

die() {
  printf 'publish-r2: %s\n' "$*" >&2
  exit 1
}

require_value() {
  local option="$1"
  local value="${2-}"
  [[ -n "$value" ]] || die "$option requires a non-empty value"
}

version=""
bucket=""
endpoint=""
base_url=""
artifacts_dir=""
manifest=""

while (( $# > 0 )); do
  case "$1" in
    --version)
      require_value "$1" "${2-}"
      version="$2"
      shift 2
      ;;
    --bucket)
      require_value "$1" "${2-}"
      bucket="$2"
      shift 2
      ;;
    --endpoint)
      require_value "$1" "${2-}"
      endpoint="$2"
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
    --manifest)
      require_value "$1" "${2-}"
      manifest="$2"
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
[[ -n "$bucket" ]] || die "--bucket is required"
[[ -n "$endpoint" ]] || die "--endpoint is required"
[[ -n "$base_url" ]] || die "--base-url is required"
[[ -n "$artifacts_dir" ]] || die "--artifacts-dir is required"
[[ -n "$manifest" ]] || die "--manifest is required"

if [[ -z "${AWS_ACCESS_KEY_ID:-}" || -z "${AWS_SECRET_ACCESS_KEY:-}" ]]; then
  die "AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY are required"
fi

if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$ ]]; then
  die "version must be SemVer without a leading v"
fi
if [[ ! "$bucket" =~ ^[a-z0-9][a-z0-9.-]{1,61}[a-z0-9]$ ]]; then
  die "bucket is not a valid R2 bucket name"
fi
if [[ ! "$endpoint" =~ ^https://[^/?#[:space:]]+/?$ ]]; then
  die "endpoint must be an HTTPS origin without a path, query, or fragment"
fi
if [[ ! "$base_url" =~ ^https://[^/?#[:space:]]+([^?#[:space:]]*)?$ ]]; then
  die "base-url must be an HTTPS origin or path without a query or fragment"
fi
[[ -d "$artifacts_dir" ]] || die "artifacts directory does not exist: $artifacts_dir"
[[ -s "$manifest" ]] || die "manifest does not exist or is empty: $manifest"

command -v aws >/dev/null 2>&1 || die "AWS CLI v2 is required"
command -v cmp >/dev/null 2>&1 || die "cmp is required"
command -v curl >/dev/null 2>&1 || die "curl is required"
command -v jq >/dev/null 2>&1 || die "jq is required"
command -v python3 >/dev/null 2>&1 || die "Python 3 is required"

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
semver_compare="$script_dir/compare-semver.py"
[[ -f "$semver_compare" ]] || die "SemVer comparison helper is missing: $semver_compare"
python3 "$semver_compare" "$version" "$version" >/dev/null \
  || die "version is not valid SemVer"

cdn_verify_attempts="${DUCKGOOKEY_CDN_VERIFY_ATTEMPTS:-6}"
cdn_verify_delay="${DUCKGOOKEY_CDN_VERIFY_DELAY_SECONDS:-5}"
[[ "$cdn_verify_attempts" =~ ^[0-9]+$ ]] \
  || die "DUCKGOOKEY_CDN_VERIFY_ATTEMPTS must be a positive integer"
[[ "$cdn_verify_delay" =~ ^[0-9]+$ ]] \
  || die "DUCKGOOKEY_CDN_VERIFY_DELAY_SECONDS must be a non-negative integer"
(( cdn_verify_attempts > 0 )) \
  || die "DUCKGOOKEY_CDN_VERIFY_ATTEMPTS must be greater than zero"

while [[ "$endpoint" == */ ]]; do
  endpoint="${endpoint%/}"
done
while [[ "$base_url" == */ ]]; do
  base_url="${base_url%/}"
done
export AWS_DEFAULT_REGION="${AWS_DEFAULT_REGION:-auto}"
export AWS_EC2_METADATA_DISABLED="${AWS_EC2_METADATA_DISABLED:-true}"

sha256_file() {
  local file="$1"
  local digest=""

  if command -v sha256sum >/dev/null 2>&1; then
    digest="$(sha256sum "$file" | awk '{print $1}')"
  elif command -v shasum >/dev/null 2>&1; then
    digest="$(shasum -a 256 "$file" | awk '{print $1}')"
  else
    die "sha256sum or shasum is required"
  fi

  [[ "$digest" =~ ^[0-9a-f]{64}$ ]] || die "failed to calculate SHA-256 for: $file"
  printf '%s\n' "$digest"
}

verify_checksum_file() {
  local checksum_file="$1"

  if command -v sha256sum >/dev/null 2>&1; then
    (
      cd "$artifacts_dir"
      sha256sum -c "$checksum_file"
    )
  elif command -v shasum >/dev/null 2>&1; then
    (
      cd "$artifacts_dir"
      shasum -a 256 -c "$checksum_file"
    )
  else
    die "sha256sum or shasum is required"
  fi
}

expected_files=(
  "DuckGooKey-$version-macos-aarch64.dmg"
  "DuckGooKey-$version-macos-aarch64.app.zip"
  "DuckGooKey-$version-macos-aarch64.SHA256SUMS"
  "DuckGooKey-$version-macos-x86_64.dmg"
  "DuckGooKey-$version-macos-x86_64.app.zip"
  "DuckGooKey-$version-macos-x86_64.SHA256SUMS"
)

for name in "${expected_files[@]}"; do
  [[ -s "$artifacts_dir/$name" ]] || die "missing or empty release artifact: $artifacts_dir/$name"
done

verify_checksum_file "DuckGooKey-$version-macos-aarch64.SHA256SUMS"
verify_checksum_file "DuckGooKey-$version-macos-x86_64.SHA256SUMS"

aarch64_dmg="DuckGooKey-$version-macos-aarch64.dmg"
x86_64_dmg="DuckGooKey-$version-macos-x86_64.dmg"
aarch64_sha256="$(sha256_file "$artifacts_dir/$aarch64_dmg")"
x86_64_sha256="$(sha256_file "$artifacts_dir/$x86_64_dmg")"
aarch64_url="$base_url/releases/v$version/$aarch64_dmg"
x86_64_url="$base_url/releases/v$version/$x86_64_dmg"

jq -e \
  --arg version "$version" \
  --arg aarch64_url "$aarch64_url" \
  --arg aarch64_sha256 "$aarch64_sha256" \
  --arg x86_64_url "$x86_64_url" \
  --arg x86_64_sha256 "$x86_64_sha256" \
  '
    .version == $version
    and (.pub_date | type == "string")
    and (.pub_date | test("^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$"))
    and (keys | sort == ["platforms", "pub_date", "version"])
    and (.platforms | keys | sort == ["macos-aarch64", "macos-x86_64"])
    and (.platforms["macos-aarch64"] | keys | sort == ["sha256", "url"])
    and .platforms["macos-aarch64"].url == $aarch64_url
    and .platforms["macos-aarch64"].sha256 == $aarch64_sha256
    and (.platforms["macos-x86_64"] | keys | sort == ["sha256", "url"])
    and .platforms["macos-x86_64"].url == $x86_64_url
    and .platforms["macos-x86_64"].sha256 == $x86_64_sha256
  ' "$manifest" >/dev/null || die "manifest does not match the release artifacts"

aws_r2() {
  aws --endpoint-url "$endpoint" --no-cli-pager "$@"
}

object_exists() {
  local key="$1"
  local listing_file="$2"

  aws_r2 s3api list-objects-v2 \
    --bucket "$bucket" \
    --prefix "$key" \
    --max-keys 1 \
    --output json > "$listing_file"

  jq -e --arg key "$key" '.Contents[]? | select(.Key == $key)' "$listing_file" >/dev/null
}

snapshot_latest() {
  local body_file="$1"
  local metadata_file="$2"
  local listing_file=""

  listing_file="$(mktemp)"
  if ! object_exists "latest.json" "$listing_file"; then
    rm -f "$listing_file" "$body_file" "$metadata_file"
    return 1
  fi
  rm -f "$listing_file"

  if ! aws_r2 s3api get-object \
    --bucket "$bucket" \
    --key "latest.json" \
    "$body_file" \
    --output json > "$metadata_file"; then
    rm -f "$body_file" "$metadata_file"
    die "latest.json existed but could not be read"
  fi
}

compare_latest_snapshot() {
  local body_file="$1"
  local existing_version=""
  local comparison=""

  existing_version="$(jq -er '.version | strings | select(length > 0)' "$body_file")" \
    || die "existing latest.json has no valid version field"
  comparison="$(python3 "$semver_compare" "$existing_version" "$version")" \
    || die "existing latest.json contains invalid SemVer: $existing_version"

  case "$comparison" in
    1)
      die "refusing to move latest.json backward from $existing_version to $version"
      ;;
    0)
      cmp -s "$body_file" "$manifest" \
        || die "latest.json already advertises version $version with different release data"
      ;;
    -1)
      ;;
    *)
      die "unexpected SemVer comparison result: $comparison"
      ;;
  esac
}

verify_public_bytes() {
  local source="$1"
  local url="$2"
  local label="$3"
  local downloaded=""
  local attempt=1

  downloaded="$(mktemp)"
  while (( attempt <= cdn_verify_attempts )); do
    if curl \
      --fail \
      --location \
      --silent \
      --show-error \
      --connect-timeout 20 \
      --max-time 900 \
      --header 'Cache-Control: no-cache' \
      --header 'Pragma: no-cache' \
      --output "$downloaded" \
      "$url" \
      && cmp -s "$source" "$downloaded"; then
      rm -f "$downloaded"
      printf 'Verified through public CDN: %s\n' "$url"
      return 0
    fi

    if (( attempt < cdn_verify_attempts )); then
      printf 'Public CDN has not served matching %s yet; retrying (%s/%s)...\n' \
        "$label" "$attempt" "$cdn_verify_attempts"
      sleep "$cdn_verify_delay"
    fi
    attempt=$((attempt + 1))
  done

  rm -f "$downloaded"
  die "public CDN verification failed for $label: $url"
}

content_type_for() {
  case "$1" in
    *.dmg)
      printf '%s\n' "application/x-apple-diskimage"
      ;;
    *.zip)
      printf '%s\n' "application/zip"
      ;;
    *.json)
      printf '%s\n' "application/json"
      ;;
    *.SHA256SUMS)
      printf '%s\n' "text/plain; charset=utf-8"
      ;;
    *)
      printf '%s\n' "application/octet-stream"
      ;;
  esac
}

verify_remote_object() {
  local key="$1"
  local expected_sha256="$2"
  local expected_size="$3"
  local head_file="$4"
  local expected_content_type="$5"
  local expected_cache_control="$6"

  aws_r2 s3api head-object \
    --bucket "$bucket" \
    --key "$key" \
    --output json > "$head_file"

  jq -e \
    --arg sha256 "$expected_sha256" \
    --argjson size "$expected_size" \
    --arg content_type "$expected_content_type" \
    --arg cache_control "$expected_cache_control" \
    '
      (.Metadata.sha256 // "") == $sha256
      and .ContentLength == $size
      and .ContentType == $content_type
      and .CacheControl == $cache_control
    ' \
    "$head_file" >/dev/null
}

upload_immutable() {
  local source="$1"
  local key="$2"
  local content_type="$3"
  local digest=""
  local size=""
  local listing_file=""
  local head_file=""

  digest="$(sha256_file "$source")"
  size="$(wc -c < "$source" | tr -d '[:space:]')"
  [[ "$size" =~ ^[0-9]+$ ]] || die "failed to determine file size: $source"

  listing_file="$(mktemp)"
  head_file="$(mktemp)"

  if object_exists "$key" "$listing_file"; then
    if verify_remote_object \
      "$key" \
      "$digest" \
      "$size" \
      "$head_file" \
      "$content_type" \
      "public, max-age=31536000, immutable"; then
      printf 'Immutable object already matches; skipping: s3://%s/%s\n' "$bucket" "$key"
      rm -f "$listing_file" "$head_file"
      return 0
    fi
    rm -f "$listing_file" "$head_file"
    die "refusing to overwrite immutable object with different or missing SHA-256 metadata: s3://$bucket/$key"
  fi

  if ! aws_r2 s3api put-object \
    --bucket "$bucket" \
    --key "$key" \
    --body "$source" \
    --content-type "$content_type" \
    --cache-control "public, max-age=31536000, immutable" \
    --metadata "sha256=$digest" \
    --if-none-match "*" \
    --output json >/dev/null; then
    # A concurrent publisher may have created the same key after the list.
    # Accept that race only when the resulting immutable object is identical.
    if object_exists "$key" "$listing_file" \
      && verify_remote_object \
        "$key" \
        "$digest" \
        "$size" \
        "$head_file" \
        "$content_type" \
        "public, max-age=31536000, immutable"; then
      printf 'Concurrent immutable upload already matches: s3://%s/%s\n' "$bucket" "$key"
      rm -f "$listing_file" "$head_file"
      return 0
    fi
    rm -f "$listing_file" "$head_file"
    die "conditional immutable upload failed: s3://$bucket/$key"
  fi

  if ! verify_remote_object \
    "$key" \
    "$digest" \
    "$size" \
    "$head_file" \
    "$content_type" \
    "public, max-age=31536000, immutable"; then
    rm -f "$listing_file" "$head_file"
    die "remote verification failed after uploading: s3://$bucket/$key"
  fi

  rm -f "$listing_file" "$head_file"
  printf 'Uploaded immutable object: s3://%s/%s\n' "$bucket" "$key"
}

# Reject an obvious downgrade before uploading versioned objects. The same
# check is repeated with an ETag immediately before the mutable pointer update
# so concurrent local and GitHub publishers cannot race latest.json.
preflight_latest_body="$(mktemp)"
preflight_latest_metadata="$(mktemp)"
if snapshot_latest "$preflight_latest_body" "$preflight_latest_metadata"; then
  compare_latest_snapshot "$preflight_latest_body"
fi
rm -f "$preflight_latest_body" "$preflight_latest_metadata"

release_prefix="releases/v$version"
for name in "${expected_files[@]}"; do
  upload_immutable \
    "$artifacts_dir/$name" \
    "$release_prefix/$name" \
    "$(content_type_for "$name")"
done

upload_immutable \
  "$manifest" \
  "$release_prefix/release.json" \
  "application/json"

# Do not advertise the release until the user-facing custom domain can serve
# both installers and the immutable release manifest byte-for-byte.
verify_public_bytes \
  "$artifacts_dir/$aarch64_dmg" \
  "$aarch64_url" \
  "$aarch64_dmg"
verify_public_bytes \
  "$artifacts_dir/$x86_64_dmg" \
  "$x86_64_url" \
  "$x86_64_dmg"
verify_public_bytes \
  "$manifest" \
  "$base_url/$release_prefix/release.json" \
  "release.json"

# This mutable pointer is deliberately the final write. A failed build,
# partial upload, downgrade, or failed CDN check cannot advertise a release.
manifest_sha256="$(sha256_file "$manifest")"
latest_size="$(wc -c < "$manifest" | tr -d '[:space:]')"
latest_updated="false"
latest_attempt=1
while (( latest_attempt <= 5 )); do
  latest_body="$(mktemp)"
  latest_metadata="$(mktemp)"
  latest_condition=()

  if snapshot_latest "$latest_body" "$latest_metadata"; then
    compare_latest_snapshot "$latest_body"
    current_etag="$(jq -er '.ETag | strings | select(length > 0)' "$latest_metadata")" \
      || die "existing latest.json has no ETag"

    if cmp -s "$latest_body" "$manifest" \
      && jq -e \
        --arg sha256 "$manifest_sha256" \
        --argjson size "$latest_size" \
        '
          (.Metadata.sha256 // "") == $sha256
          and .ContentLength == $size
          and .ContentType == "application/json"
          and .CacheControl == "no-cache, no-store, must-revalidate"
        ' \
        "$latest_metadata" >/dev/null; then
      rm -f "$latest_body" "$latest_metadata"
      latest_updated="true"
      printf 'latest.json already matches this release; skipping pointer update.\n'
      break
    fi
    latest_condition=(--if-match "$current_etag")
  else
    latest_condition=(--if-none-match "*")
  fi

  rm -f "$latest_body" "$latest_metadata"
  if aws_r2 s3api put-object \
    --bucket "$bucket" \
    --key "latest.json" \
    --body "$manifest" \
    --content-type "application/json" \
    --cache-control "no-cache, no-store, must-revalidate" \
    --metadata "sha256=$manifest_sha256" \
    "${latest_condition[@]}" \
    --output json >/dev/null; then
    latest_updated="true"
    break
  fi

  printf 'latest.json changed concurrently; rechecking release ordering (%s/5)...\n' \
    "$latest_attempt"
  latest_attempt=$((latest_attempt + 1))
done

[[ "$latest_updated" == "true" ]] \
  || die "could not update latest.json after concurrent changes"

latest_head="$(mktemp)"
if ! verify_remote_object \
  "latest.json" \
  "$manifest_sha256" \
  "$latest_size" \
  "$latest_head" \
  "application/json" \
  "no-cache, no-store, must-revalidate"; then
  rm -f "$latest_head"
  die "latest.json verification failed after publishing"
fi
rm -f "$latest_head"

verify_public_bytes \
  "$manifest" \
  "$base_url/latest.json" \
  "latest.json"

printf 'Published latest manifest: %s/latest.json\n' "$base_url"
