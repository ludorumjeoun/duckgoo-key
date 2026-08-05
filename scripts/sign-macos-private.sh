#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  sign-macos-private.sh \
    --app APP_BUNDLE \
    --dmg DMG_FILE \
    --p12 PRIVATE_IDENTITY_P12 \
    --expected-cert-sha256 SHA256 \
    --certificate-out PUBLIC_CERTIFICATE_CER \
    --metadata-out SIGNING_METADATA_TXT

Required environment:
  DUCKGOOKEY_PRIVATE_SIGNING_PASSWORD

Imports the stable private identity into a temporary keychain, trusts it only
for the duration of this command, signs the app inside-out, rebuilds the DMG
from that signed app, signs the DMG, verifies both signatures, then removes the
temporary keychain and temporary trust setting. Apple notarization is not used.
EOF
}

die() {
  printf 'sign-macos-private: %s\n' "$*" >&2
  exit 1
}

app_path=""
dmg_path=""
p12_path=""
expected_certificate_sha256=""
certificate_out=""
metadata_out=""

while (( $# > 0 )); do
  case "$1" in
    --app)
      [[ -n "${2-}" ]] || die "--app requires a bundle path"
      app_path="$2"
      shift 2
      ;;
    --dmg)
      [[ -n "${2-}" ]] || die "--dmg requires a file path"
      dmg_path="$2"
      shift 2
      ;;
    --p12)
      [[ -n "${2-}" ]] || die "--p12 requires a file path"
      p12_path="$2"
      shift 2
      ;;
    --expected-cert-sha256)
      [[ -n "${2-}" ]] || die "--expected-cert-sha256 requires a fingerprint"
      expected_certificate_sha256="$(printf '%s' "$2" | tr -d '[:space:]:' | tr '[:upper:]' '[:lower:]')"
      shift 2
      ;;
    --certificate-out)
      [[ -n "${2-}" ]] || die "--certificate-out requires a file path"
      certificate_out="$2"
      shift 2
      ;;
    --metadata-out)
      [[ -n "${2-}" ]] || die "--metadata-out requires a file path"
      metadata_out="$2"
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

[[ "$(uname -s)" == "Darwin" ]] || die "private macOS signing requires macOS"
[[ -d "$app_path/Contents" ]] || die "app bundle does not exist: $app_path"
[[ -f "$dmg_path" && "$dmg_path" == *.dmg ]] || die "DMG does not exist or lacks a .dmg extension: $dmg_path"
[[ -f "$p12_path" ]] || die "P12 does not exist: $p12_path"
[[ "$expected_certificate_sha256" =~ ^[0-9a-f]{64}$ ]] || \
  die "expected certificate SHA-256 must contain 64 hexadecimal characters"
[[ -n "$certificate_out" ]] || die "--certificate-out is required"
[[ -n "$metadata_out" ]] || die "--metadata-out is required"
[[ -n "${DUCKGOOKEY_PRIVATE_SIGNING_PASSWORD:-}" ]] || \
  die "DUCKGOOKEY_PRIVATE_SIGNING_PASSWORD is required"

for command in awk codesign ditto file find grep hdiutil openssl security sed shasum tr xattr; do
  command -v "$command" >/dev/null 2>&1 || die "$command is required"
done

temporary_root="${TMPDIR:-/tmp}"
temporary_root="${temporary_root%/}"
temporary_dir="$(mktemp -d "$temporary_root/duckgookey-private-sign.XXXXXX")"
leaf_pem="$temporary_dir/private-signing.cert.pem"
leaf_der="$temporary_dir/private-signing.cer"
original_keychains="$temporary_dir/original-keychains.txt"
keychain_id="duckgookey-private-$$-$RANDOM.keychain"
keychain_password="$(openssl rand -hex 32)"
trust_added="false"
keychain_created="false"

security list-keychains -d user > "$original_keychains"

restore_keychain_search_list() {
  local restored=()
  local entry=""

  while IFS= read -r entry; do
    entry="$(printf '%s\n' "$entry" | sed -E 's/^[[:space:]]*"//; s/"[[:space:]]*$//')"
    [[ -n "$entry" ]] && restored+=("$entry")
  done < "$original_keychains"

  if (( ${#restored[@]} > 0 )); then
    security list-keychains -d user -s "${restored[@]}" >/dev/null 2>&1 || true
  fi
}

cleanup() {
  if [[ "$trust_added" == "true" && -s "$leaf_pem" ]]; then
    security remove-trusted-cert "$leaf_pem" >/dev/null 2>&1 || true
  fi
  restore_keychain_search_list
  if [[ "$keychain_created" == "true" ]]; then
    security delete-keychain "$keychain_id" >/dev/null 2>&1 || true
  fi
  rm -rf "$temporary_dir"
}
trap cleanup EXIT

openssl pkcs12 \
  -in "$p12_path" \
  -clcerts \
  -nokeys \
  -passin env:DUCKGOOKEY_PRIVATE_SIGNING_PASSWORD \
  -out "$leaf_pem"
[[ -s "$leaf_pem" ]] || die "P12 does not contain a leaf certificate"
openssl x509 -in "$leaf_pem" -outform DER -out "$leaf_der"

actual_certificate_sha256="$(shasum -a 256 "$leaf_der" | awk '{print $1}')"
identity_sha1="$(
  openssl x509 -in "$leaf_pem" -noout -fingerprint -sha1 |
    awk -F= '{gsub(":", "", $2); print toupper($2)}'
)"
[[ "$actual_certificate_sha256" == "$expected_certificate_sha256" ]] || \
  die "P12 certificate SHA-256 is $actual_certificate_sha256; expected $expected_certificate_sha256"
[[ "$identity_sha1" =~ ^[0-9A-F]{40}$ ]] || die "failed to calculate certificate SHA-1"

security create-keychain -p "$keychain_password" "$keychain_id"
keychain_created="true"
security set-keychain-settings -lut 21600 "$keychain_id"
security unlock-keychain -p "$keychain_password" "$keychain_id"
security import "$p12_path" \
  -k "$keychain_id" \
  -f pkcs12 \
  -P "$DUCKGOOKEY_PRIVATE_SIGNING_PASSWORD" \
  -T /usr/bin/codesign \
  -T /usr/bin/security >/dev/null
security set-key-partition-list \
  -S "apple-tool:,apple:,codesign:" \
  -s \
  -k "$keychain_password" \
  "$keychain_id" >/dev/null

current_keychains=()
while IFS= read -r entry; do
  entry="$(printf '%s\n' "$entry" | sed -E 's/^[[:space:]]*"//; s/"[[:space:]]*$//')"
  [[ -n "$entry" ]] && current_keychains+=("$entry")
done < "$original_keychains"
current_keychains+=("$keychain_id")
security list-keychains -d user -s "${current_keychains[@]}"

if ! security verify-cert -c "$leaf_pem" -p codeSign -k "$keychain_id" -q >/dev/null 2>&1; then
  security add-trusted-cert \
    -r trustRoot \
    -p codeSign \
    -k "$keychain_id" \
    "$leaf_pem"
  trust_added="true"
fi

if ! security find-identity -v -p codesigning "$keychain_id" |
  awk -v expected="$identity_sha1" '$2 == expected {found = 1} END {exit found ? 0 : 1}'; then
  die "certificate $identity_sha1 is not a valid code-signing identity"
fi

unset DUCKGOOKEY_PRIVATE_SIGNING_PASSWORD
xattr -cr "$app_path"

if find "$app_path/Contents" -mindepth 2 -type d \
  \( -name '*.app' -o -name '*.appex' -o -name '*.framework' -o -name '*.plugin' -o -name '*.xpc' \) \
  -print -quit | grep -q .; then
  die "nested code bundles were found; update the inside-out signing policy before releasing"
fi

signed_macho_count=0
while IFS= read -r -d '' candidate; do
  if file -b "$candidate" | grep -q 'Mach-O'; then
    codesign \
      --force \
      --options runtime \
      --timestamp=none \
      --keychain "$keychain_id" \
      --sign "$identity_sha1" \
      "$candidate"
    signed_macho_count=$((signed_macho_count + 1))
  fi
done < <(find "$app_path/Contents" -type f -print0)
(( signed_macho_count > 0 )) || die "no Mach-O executable was found in the app bundle"

codesign \
  --force \
  --options runtime \
  --timestamp=none \
  --keychain "$keychain_id" \
  --sign "$identity_sha1" \
  "$app_path"
codesign --verify --deep --strict --verbose=2 "$app_path"

extract_embedded_certificate_sha256() {
  local signed_path="$1"
  rm -f "$temporary_dir/codesign0" "$temporary_dir/codesign1" "$temporary_dir/codesign2"
  (
    cd "$temporary_dir"
    codesign --display --extract-certificates "$signed_path" >/dev/null 2>&1
  )
  [[ -s "$temporary_dir/codesign0" ]] || die "could not extract embedded signing certificate from $signed_path"
  shasum -a 256 "$temporary_dir/codesign0" | awk '{print $1}'
}

app_certificate_sha256="$(extract_embedded_certificate_sha256 "$app_path")"
[[ "$app_certificate_sha256" == "$expected_certificate_sha256" ]] || \
  die "app embedded certificate does not match the pinned private certificate"

staging_dir="$temporary_dir/disk"
temporary_dmg="$temporary_dir/$(basename "$dmg_path")"
mkdir -p "$staging_dir"
ditto "$app_path" "$staging_dir/$(basename "$app_path")"
ln -s /Applications "$staging_dir/Applications"
hdiutil create \
  -volname "DuckGooKey Private" \
  -srcfolder "$staging_dir" \
  -format UDZO \
  -ov \
  "$temporary_dmg" >/dev/null

codesign \
  --force \
  --timestamp=none \
  --keychain "$keychain_id" \
  --sign "$identity_sha1" \
  "$temporary_dmg"
hdiutil verify "$temporary_dmg" >/dev/null
codesign --verify --strict --verbose=2 "$temporary_dmg"

dmg_certificate_sha256="$(extract_embedded_certificate_sha256 "$temporary_dmg")"
[[ "$dmg_certificate_sha256" == "$expected_certificate_sha256" ]] || \
  die "DMG embedded certificate does not match the pinned private certificate"

mkdir -p "$(dirname "$dmg_path")" "$(dirname "$certificate_out")" "$(dirname "$metadata_out")"
mv -f "$temporary_dmg" "$dmg_path"
cp "$leaf_der" "$certificate_out"
chmod 644 "$certificate_out"

certificate_subject="$(openssl x509 -in "$leaf_pem" -noout -subject -nameopt RFC2253 | sed 's/^subject=//')"
certificate_not_before="$(openssl x509 -in "$leaf_pem" -noout -startdate | sed 's/^notBefore=//')"
certificate_not_after="$(openssl x509 -in "$leaf_pem" -noout -enddate | sed 's/^notAfter=//')"
certificate_sha256_colon="$(openssl x509 -in "$leaf_pem" -noout -fingerprint -sha256 | sed 's/^.*=//')"

cat > "$metadata_out" <<EOF
DuckGooKey private distribution signature

Certificate subject: $certificate_subject
Certificate SHA-256: $certificate_sha256_colon
Valid from: $certificate_not_before
Valid until: $certificate_not_after

The app and DMG are signed with this pinned private certificate. They are not
signed with Apple Developer ID and are not notarized by Apple. Verify this
fingerprint through a separate trusted channel before trusting the certificate.
EOF
chmod 644 "$metadata_out"

printf 'Private-signed DuckGooKey packages are ready:\n'
printf '  App: %s\n' "$app_path"
printf '  DMG: %s\n' "$dmg_path"
printf '  Public certificate: %s\n' "$certificate_out"
printf '  Certificate SHA-256: %s\n' "$actual_certificate_sha256"
