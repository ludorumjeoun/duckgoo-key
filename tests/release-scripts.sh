#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
project_dir="$(cd "$script_dir/.." && pwd)"
temporary_dir="$(mktemp -d "${TMPDIR:-/tmp}/duckgookey-release-tests.XXXXXX")"
cleanup() {
  rm -rf "$temporary_dir"
}
trap cleanup EXIT

fail() {
  printf 'release-scripts test: %s\n' "$*" >&2
  exit 1
}

assert_compare() {
  local expected="$1"
  local left="$2"
  local right="$3"
  local actual=""

  actual="$(python3 "$project_dir/scripts/compare-semver.py" "$left" "$right")"
  [[ "$actual" == "$expected" ]] \
    || fail "expected $left compared with $right to be $expected, got $actual"
}

assert_compare -1 1.0.0-alpha 1.0.0-alpha.1
assert_compare -1 1.0.0-alpha.1 1.0.0-alpha.beta
assert_compare -1 1.0.0-beta.2 1.0.0-beta.11
assert_compare -1 1.0.0-rc.1 1.0.0
assert_compare 0 1.0.0+local 1.0.0+remote
assert_compare 1 2.0.0 1.99.99

if python3 "$project_dir/scripts/compare-semver.py" 01.0.0 1.0.0 >/dev/null 2>&1; then
  fail "SemVer comparison accepted a leading-zero major version"
fi

"$project_dir/scripts/package-macos-release-arch.sh" --help >/dev/null
"$project_dir/scripts/release-local.sh" --help >/dev/null
"$project_dir/scripts/publish-r2.sh" --help >/dev/null
"$project_dir/scripts/release-configure.sh" --help >/dev/null

keychain_dir="$temporary_dir/fake-keychain"
keychain_log="$temporary_dir/fake-keychain.log"
fake_mise_log="$temporary_dir/fake-mise.log"
mkdir -p "$keychain_dir/items"
: > "$keychain_log"
: > "$fake_mise_log"

if ! git -C "$project_dir" check-ignore -q mise.local.toml; then
  fail "machine-local Mise release configuration is not ignored"
fi
if git -C "$project_dir" check-ignore -q mise.local.example.toml; then
  fail "the committed Mise local configuration example is ignored"
fi
if rg -q 'CLOUDFLARE_R2_(ACCESS_KEY_ID|SECRET_ACCESS_KEY)|PASSWORD|AWS_ACCESS' \
  "$project_dir/mise.local.example.toml"; then
  fail "Mise local configuration example contains a credential field"
fi
python3 - "$project_dir/mise.local.example.toml" <<'PY'
import pathlib
import sys
import tomllib

data = tomllib.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
assert data["env"]["APPLE_TEAM_ID"]["default"] == "TEAMID"
assert data["env"]["CLOUDFLARE_R2_BUCKET"]["default"] == "duckgoo-key-releases"
assert data["env"]["CLOUDFLARE_R2_PUBLIC_BASE_URL"]["default"] == "https://updates.key.duckgoo.net"
PY
grep -qF 'raw = true' "$project_dir/mise.toml" \
  || fail "release configuration task is not raw and cannot prompt reliably"

# The complete environment pair wins without touching Keychain.
(
  export PATH="$project_dir/tests/fakes:$PATH"
  export FAKE_SECURITY_DIR="$keychain_dir"
  export FAKE_SECURITY_LOG="$keychain_log"
  export CLOUDFLARE_R2_ACCESS_KEY_ID="environment-access-key"
  export CLOUDFLARE_R2_SECRET_ACCESS_KEY="environment-secret-key"
  source "$project_dir/scripts/release-keychain.sh"
  duckgookey_resolve_r2_credentials
  [[ "$DUCKGOOKEY_RESOLVED_R2_ACCESS_KEY_ID" == "environment-access-key" ]]
  [[ "$DUCKGOOKEY_RESOLVED_R2_SECRET_ACCESS_KEY" == "environment-secret-key" ]]
)
[[ ! -s "$keychain_log" ]] \
  || fail "complete environment credentials unexpectedly read Keychain"

# A partial override must not be combined with one Keychain value.
if (
  export PATH="$project_dir/tests/fakes:$PATH"
  export FAKE_SECURITY_DIR="$keychain_dir"
  export FAKE_SECURITY_LOG="$keychain_log"
  export CLOUDFLARE_R2_ACCESS_KEY_ID="environment-access-key"
  unset CLOUDFLARE_R2_SECRET_ACCESS_KEY
  source "$project_dir/scripts/release-keychain.sh"
  duckgookey_resolve_r2_credentials
); then
  fail "partial environment credentials were accepted"
fi
[[ ! -s "$keychain_log" ]] \
  || fail "partial environment credentials unexpectedly read Keychain"

# A missing environment pair reads the two exact Keychain entries once.
printf 'keychain-access-key' > "$keychain_dir/items/CLOUDFLARE_R2_ACCESS_KEY_ID"
printf 'keychain-secret-key' > "$keychain_dir/items/CLOUDFLARE_R2_SECRET_ACCESS_KEY"
(
  export PATH="$project_dir/tests/fakes:$PATH"
  export FAKE_SECURITY_DIR="$keychain_dir"
  export FAKE_SECURITY_LOG="$keychain_log"
  unset CLOUDFLARE_R2_ACCESS_KEY_ID CLOUDFLARE_R2_SECRET_ACCESS_KEY
  source "$project_dir/scripts/release-keychain.sh"
  duckgookey_resolve_r2_credentials
  [[ "$DUCKGOOKEY_RESOLVED_R2_ACCESS_KEY_ID" == "keychain-access-key" ]]
  [[ "$DUCKGOOKEY_RESOLVED_R2_SECRET_ACCESS_KEY" == "keychain-secret-key" ]]
)
[[ "$(grep -c '^find com.duckgoo.key.release.r2 .* true$' "$keychain_log")" == "2" ]] \
  || fail "Keychain fallback did not read exactly two credential items"

rm "$keychain_dir/items/CLOUDFLARE_R2_SECRET_ACCESS_KEY"
if (
  export PATH="$project_dir/tests/fakes:$PATH"
  export FAKE_SECURITY_DIR="$keychain_dir"
  export FAKE_SECURITY_LOG="$keychain_log"
  unset CLOUDFLARE_R2_ACCESS_KEY_ID CLOUDFLARE_R2_SECRET_ACCESS_KEY
  source "$project_dir/scripts/release-keychain.sh"
  duckgookey_resolve_r2_credentials
); then
  fail "missing Keychain secret was accepted"
fi
printf 'keychain-secret-key' > "$keychain_dir/items/CLOUDFLARE_R2_SECRET_ACCESS_KEY"

configured_local_file="$temporary_dir/mise.local.toml"
configure_output="$temporary_dir/release-configure.out"
rm "$keychain_dir/items/CLOUDFLARE_R2_ACCESS_KEY_ID" \
  "$keychain_dir/items/CLOUDFLARE_R2_SECRET_ACCESS_KEY"
(
  printf '\n\n\n\n\n\n\nfixture-access-key\nfixture-secret-key\n' \
    | env \
      PATH="$project_dir/tests/fakes:$PATH" \
      FAKE_SECURITY_DIR="$keychain_dir" \
      FAKE_SECURITY_LOG="$keychain_log" \
      FAKE_SECURITY_IDENTITIES='Developer ID Application: DuckGooKey Test (ABCDE12345)' \
      FAKE_MISE_LOG="$fake_mise_log" \
      APPLE_SIGNING_IDENTITY='Developer ID Application: DuckGooKey Test (ABCDE12345)' \
      APPLE_TEAM_ID='ABCDE12345' \
      APPLE_KEYCHAIN_PROFILE='duckgookey-notary' \
      CLOUDFLARE_R2_BUCKET='duckgoo-key-releases' \
      CLOUDFLARE_R2_ENDPOINT='https://0123456789abcdef0123456789abcdef.r2.cloudflarestorage.com' \
      CLOUDFLARE_R2_PUBLIC_BASE_URL='https://updates.key.duckgoo.net' \
      "$project_dir/scripts/release-configure.sh" \
        --config "$configured_local_file"
) > "$configure_output" 2>&1

python3 - "$configured_local_file" <<'PY'
import pathlib
import sys
import tomllib

data = tomllib.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
env = data["env"]
assert env["APPLE_SIGNING_IDENTITY"]["default"] == "Developer ID Application: DuckGooKey Test (ABCDE12345)"
assert env["APPLE_TEAM_ID"]["default"] == "ABCDE12345"
assert env["APPLE_KEYCHAIN_PROFILE"]["default"] == "duckgookey-notary"
assert env["CLOUDFLARE_R2_BUCKET"]["default"] == "duckgoo-key-releases"
assert env["CLOUDFLARE_R2_ENDPOINT"]["default"].startswith("https://0123456789abcdef")
assert env["CLOUDFLARE_R2_PUBLIC_BASE_URL"]["default"] == "https://updates.key.duckgoo.net"
assert "CLOUDFLARE_R2_ACCESS_KEY_ID" not in env
assert "CLOUDFLARE_R2_SECRET_ACCESS_KEY" not in env
PY
[[ "$(cat "$keychain_dir/items/CLOUDFLARE_R2_ACCESS_KEY_ID")" == "fixture-access-key" ]] \
  || fail "release configurator did not store the R2 access key via Keychain"
[[ "$(cat "$keychain_dir/items/CLOUDFLARE_R2_SECRET_ACCESS_KEY")" == "fixture-secret-key" ]] \
  || fail "release configurator did not store the R2 secret via Keychain"
if rg -q 'fixture-(access|secret)-key' \
  "$configure_output" "$configured_local_file" "$keychain_log" "$fake_mise_log"; then
  fail "release configuration leaked a test credential outside fake Keychain state"
fi
grep -q '^add com.duckgoo.key.release.r2 CLOUDFLARE_R2_ACCESS_KEY_ID prompted$' "$keychain_log" \
  || fail "release configurator did not use a prompted Keychain access key write"
grep -q '^add com.duckgoo.key.release.r2 CLOUDFLARE_R2_SECRET_ACCESS_KEY prompted$' "$keychain_log" \
  || fail "release configurator did not use a prompted Keychain secret write"
grep -q '^install$' "$fake_mise_log" \
  || fail "release configurator did not prepare the pinned AWS CLI for validation"
grep -q '^aws$' "$fake_mise_log" \
  || fail "release configurator did not validate access with the AWS CLI"
grep -q '^trust$' "$fake_mise_log" \
  || fail "release configurator did not trust its completed local Mise file"

unmanaged_local_file="$temporary_dir/unmanaged.local.toml"
printf '[env]\nUNRELATED = "value"\n' > "$unmanaged_local_file"
if env \
  PATH="$project_dir/tests/fakes:$PATH" \
  FAKE_SECURITY_DIR="$keychain_dir" \
  FAKE_SECURITY_LOG="$keychain_log" \
  FAKE_SECURITY_IDENTITIES='Developer ID Application: DuckGooKey Test (ABCDE12345)' \
  FAKE_MISE_LOG="$fake_mise_log" \
  "$project_dir/scripts/release-configure.sh" \
    --config "$unmanaged_local_file" \
    --skip-r2-credentials </dev/null >/dev/null 2>&1; then
  fail "release configurator replaced an unmanaged local Mise file"
fi
grep -q '^UNRELATED = "value"$' "$unmanaged_local_file" \
  || fail "unmanaged local Mise file changed after refusal"

grep -qF './scripts/package-macos-release-arch.sh' \
  "$project_dir/.github/workflows/release.yml" \
  || fail "GitHub release does not use the shared architecture packager"
grep -qF 'package-macos-release-arch.sh' "$project_dir/scripts/release-local.sh" \
  || fail "local release does not use the shared architecture packager"
grep -qF 'publish-r2.sh' "$project_dir/scripts/release-local.sh" \
  || fail "local release cannot invoke the shared R2 publisher"
grep -qF 'release-keychain.sh' "$project_dir/scripts/release-local.sh" \
  || fail "local release does not use the shared Keychain credential resolver"
if grep -Eq 'notarytool submit|codesign --verify' \
  "$project_dir/.github/workflows/release.yml"; then
  fail "GitHub workflow duplicates public packaging or notarization logic"
fi

if "$project_dir/scripts/package-macos-release-arch.sh" \
  --distribution public-notarized \
  --version invalid \
  --platform macos-aarch64 \
  --stage-dir "$temporary_dir/stage" >/dev/null 2>&1; then
  fail "per-architecture packager accepted an invalid version"
fi

artifacts_dir="$temporary_dir/artifacts"
mkdir -p "$artifacts_dir"
printf 'arm64 installer\n' > "$artifacts_dir/DuckGooKey-1.2.3-macos-aarch64.dmg"
printf 'Intel installer\n' > "$artifacts_dir/DuckGooKey-1.2.3-macos-x86_64.dmg"

"$project_dir/scripts/build-release-manifest.sh" \
  --version 1.2.3 \
  --pub-date 2026-08-05T12:34:56Z \
  --base-url https://updates.key.duckgoo.net/ \
  --artifacts-dir "$artifacts_dir" \
  --output "$artifacts_dir/latest.json" >/dev/null

jq -e '
  .version == "1.2.3"
  and .pub_date == "2026-08-05T12:34:56Z"
  and .platforms["macos-aarch64"].url
    == "https://updates.key.duckgoo.net/releases/v1.2.3/DuckGooKey-1.2.3-macos-aarch64.dmg"
  and .platforms["macos-x86_64"].url
    == "https://updates.key.duckgoo.net/releases/v1.2.3/DuckGooKey-1.2.3-macos-x86_64.dmg"
' "$artifacts_dir/latest.json" >/dev/null \
  || fail "release manifest did not preserve the public artifact contract"

printf 'arm64 application\n' > "$artifacts_dir/DuckGooKey-1.2.3-macos-aarch64.app.zip"
printf 'Intel application\n' > "$artifacts_dir/DuckGooKey-1.2.3-macos-x86_64.app.zip"
(
  cd "$artifacts_dir"
  shasum -a 256 \
    DuckGooKey-1.2.3-macos-aarch64.dmg \
    DuckGooKey-1.2.3-macos-aarch64.app.zip \
    > DuckGooKey-1.2.3-macos-aarch64.SHA256SUMS
  shasum -a 256 \
    DuckGooKey-1.2.3-macos-x86_64.dmg \
    DuckGooKey-1.2.3-macos-x86_64.app.zip \
    > DuckGooKey-1.2.3-macos-x86_64.SHA256SUMS
)

if env -u AWS_ACCESS_KEY_ID -u AWS_SECRET_ACCESS_KEY \
  "$project_dir/scripts/publish-r2.sh" \
    --version 1.2.3 \
    --bucket duckgookey-releases \
    --endpoint https://example.r2.cloudflarestorage.com \
    --base-url https://updates.key.duckgoo.net \
    --artifacts-dir "$artifacts_dir" \
    --manifest "$artifacts_dir/latest.json" >/dev/null 2>&1; then
  fail "R2 publisher reported success without credentials"
fi

fake_r2_dir="$temporary_dir/fake-r2"
fake_r2_log="$temporary_dir/fake-r2.log"
mkdir -p "$fake_r2_dir"

run_fake_publisher() {
  local release_version="$1"
  local release_dir="$2"

  env \
    AWS_ACCESS_KEY_ID=test-access-key \
    AWS_SECRET_ACCESS_KEY=test-secret-key \
    DUCKGOOKEY_CDN_VERIFY_ATTEMPTS=1 \
    DUCKGOOKEY_CDN_VERIFY_DELAY_SECONDS=0 \
    FAKE_R2_DIR="$fake_r2_dir" \
    FAKE_R2_LOG="$fake_r2_log" \
    PATH="$project_dir/tests/fakes:$PATH" \
    "$project_dir/scripts/publish-r2.sh" \
      --version "$release_version" \
      --bucket duckgookey-releases \
      --endpoint https://example.r2.cloudflarestorage.com \
      --base-url https://updates.key.duckgoo.net \
      --artifacts-dir "$release_dir" \
      --manifest "$release_dir/latest.json" >/dev/null
}

run_fake_publisher 1.2.3 "$artifacts_dir"

installer_cdn_line="$(awk '/^curl releases\/v1.2.3\/DuckGooKey-1.2.3-macos-aarch64.dmg$/ { print NR; exit }' "$fake_r2_log")"
latest_put_line="$(awk '/^put latest.json / { print NR; exit }' "$fake_r2_log")"
latest_cdn_line="$(awk '/^curl latest.json$/ { print NR; exit }' "$fake_r2_log")"
[[ -n "$installer_cdn_line" && -n "$latest_put_line" && -n "$latest_cdn_line" ]] \
  || fail "publisher did not exercise the CDN and latest.json path"
(( installer_cdn_line < latest_put_line )) \
  || fail "publisher advertised latest.json before verifying installers through the CDN"
(( latest_put_line < latest_cdn_line )) \
  || fail "publisher did not verify latest.json after its conditional update"
grep -q '^put latest.json if-none-match=\*$' "$fake_r2_log" \
  || fail "first latest.json publication was not conditional"
jq -e '
  .ContentType == "application/x-apple-diskimage"
  and .CacheControl == "public, max-age=31536000, immutable"
' \
  "$fake_r2_dir/metadata/releases/v1.2.3/DuckGooKey-1.2.3-macos-aarch64.dmg.json" \
  >/dev/null || fail "immutable installer metadata is not cache-safe"
jq -e '
  .ContentType == "application/json"
  and .CacheControl == "no-cache, no-store, must-revalidate"
' "$fake_r2_dir/metadata/latest.json.json" >/dev/null \
  || fail "latest.json metadata permits stale caching"

run_fake_publisher 1.2.3 "$artifacts_dir"
latest_put_count="$(grep -c '^put latest.json ' "$fake_r2_log")"
[[ "$latest_put_count" == "1" ]] \
  || fail "an identical release rewrote latest.json"

upgrade_dir="$temporary_dir/upgrade"
mkdir -p "$upgrade_dir"
for platform in macos-aarch64 macos-x86_64; do
  printf '%s installer upgrade\n' "$platform" > "$upgrade_dir/DuckGooKey-1.2.4-$platform.dmg"
  printf '%s application upgrade\n' "$platform" > "$upgrade_dir/DuckGooKey-1.2.4-$platform.app.zip"
  (
    cd "$upgrade_dir"
    shasum -a 256 \
      "DuckGooKey-1.2.4-$platform.dmg" \
      "DuckGooKey-1.2.4-$platform.app.zip" \
      > "DuckGooKey-1.2.4-$platform.SHA256SUMS"
  )
done
"$project_dir/scripts/build-release-manifest.sh" \
  --version 1.2.4 \
  --pub-date 2026-08-06T12:34:56Z \
  --base-url https://updates.key.duckgoo.net \
  --artifacts-dir "$upgrade_dir" \
  --output "$upgrade_dir/latest.json" >/dev/null

run_fake_publisher 1.2.4 "$upgrade_dir"
grep -q '^put latest.json if-match=' "$fake_r2_log" \
  || fail "latest.json upgrade did not use an ETag condition"
jq -e '.version == "1.2.4"' "$fake_r2_dir/objects/latest.json" >/dev/null \
  || fail "latest.json did not advance to the newer release"

same_version_dir="$temporary_dir/same-version-different-manifest"
mkdir -p "$same_version_dir"
cp "$upgrade_dir"/DuckGooKey-1.2.4-* "$same_version_dir/"
jq '.pub_date = "2026-08-06T12:35:00Z"' \
  "$upgrade_dir/latest.json" > "$same_version_dir/latest.json"
if env \
  AWS_ACCESS_KEY_ID=test-access-key \
  AWS_SECRET_ACCESS_KEY=test-secret-key \
  DUCKGOOKEY_CDN_VERIFY_ATTEMPTS=1 \
  DUCKGOOKEY_CDN_VERIFY_DELAY_SECONDS=0 \
  FAKE_R2_DIR="$fake_r2_dir" \
  FAKE_R2_LOG="$fake_r2_log" \
  PATH="$project_dir/tests/fakes:$PATH" \
  "$project_dir/scripts/publish-r2.sh" \
    --version 1.2.4 \
    --bucket duckgookey-releases \
    --endpoint https://example.r2.cloudflarestorage.com \
    --base-url https://updates.key.duckgoo.net \
    --artifacts-dir "$same_version_dir" \
    --manifest "$same_version_dir/latest.json" >/dev/null 2>&1; then
  fail "publisher accepted different release data for an existing version"
fi

downgrade_dir="$temporary_dir/downgrade"
mkdir -p "$downgrade_dir"
for platform in macos-aarch64 macos-x86_64; do
  printf '%s installer\n' "$platform" > "$downgrade_dir/DuckGooKey-1.2.2-$platform.dmg"
  printf '%s application\n' "$platform" > "$downgrade_dir/DuckGooKey-1.2.2-$platform.app.zip"
  (
    cd "$downgrade_dir"
    shasum -a 256 \
      "DuckGooKey-1.2.2-$platform.dmg" \
      "DuckGooKey-1.2.2-$platform.app.zip" \
      > "DuckGooKey-1.2.2-$platform.SHA256SUMS"
  )
done
"$project_dir/scripts/build-release-manifest.sh" \
  --version 1.2.2 \
  --pub-date 2026-08-04T12:34:56Z \
  --base-url https://updates.key.duckgoo.net \
  --artifacts-dir "$downgrade_dir" \
  --output "$downgrade_dir/latest.json" >/dev/null

if env \
  AWS_ACCESS_KEY_ID=test-access-key \
  AWS_SECRET_ACCESS_KEY=test-secret-key \
  DUCKGOOKEY_CDN_VERIFY_ATTEMPTS=1 \
  DUCKGOOKEY_CDN_VERIFY_DELAY_SECONDS=0 \
  FAKE_R2_DIR="$fake_r2_dir" \
  FAKE_R2_LOG="$fake_r2_log" \
  PATH="$project_dir/tests/fakes:$PATH" \
  "$project_dir/scripts/publish-r2.sh" \
    --version 1.2.2 \
    --bucket duckgookey-releases \
    --endpoint https://example.r2.cloudflarestorage.com \
    --base-url https://updates.key.duckgoo.net \
    --artifacts-dir "$downgrade_dir" \
    --manifest "$downgrade_dir/latest.json" >/dev/null 2>&1; then
  fail "publisher allowed latest.json to move to an older SemVer"
fi
if grep -q '^put releases/v1.2.2/' "$fake_r2_log"; then
  fail "publisher uploaded downgrade artifacts before rejecting the release"
fi

printf 'Release script policy tests passed.\n'
