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
latest.json is overwritten only after every immutable object is present.
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
command -v jq >/dev/null 2>&1 || die "jq is required"

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

  aws_r2 s3api head-object \
    --bucket "$bucket" \
    --key "$key" \
    --output json > "$head_file"

  jq -e \
    --arg sha256 "$expected_sha256" \
    --argjson size "$expected_size" \
    '(.Metadata.sha256 // "") == $sha256 and .ContentLength == $size' \
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
    if verify_remote_object "$key" "$digest" "$size" "$head_file"; then
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
      && verify_remote_object "$key" "$digest" "$size" "$head_file"; then
      printf 'Concurrent immutable upload already matches: s3://%s/%s\n' "$bucket" "$key"
      rm -f "$listing_file" "$head_file"
      return 0
    fi
    rm -f "$listing_file" "$head_file"
    die "conditional immutable upload failed: s3://$bucket/$key"
  fi

  if ! verify_remote_object "$key" "$digest" "$size" "$head_file"; then
    rm -f "$listing_file" "$head_file"
    die "remote verification failed after uploading: s3://$bucket/$key"
  fi

  rm -f "$listing_file" "$head_file"
  printf 'Uploaded immutable object: s3://%s/%s\n' "$bucket" "$key"
}

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

# This mutable pointer is deliberately the final write. A failed build or
# partial immutable upload can therefore never advertise an incomplete release.
manifest_sha256="$(sha256_file "$manifest")"
aws_r2 s3api put-object \
  --bucket "$bucket" \
  --key "latest.json" \
  --body "$manifest" \
  --content-type "application/json" \
  --cache-control "no-cache, no-store, must-revalidate" \
  --metadata "sha256=$manifest_sha256" \
  --output json >/dev/null

latest_head="$(mktemp)"
latest_size="$(wc -c < "$manifest" | tr -d '[:space:]')"
if ! verify_remote_object "latest.json" "$manifest_sha256" "$latest_size" "$latest_head"; then
  rm -f "$latest_head"
  die "latest.json verification failed after publishing"
fi
rm -f "$latest_head"

printf 'Published latest manifest: %s/latest.json\n' "$base_url"
