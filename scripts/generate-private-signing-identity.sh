#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: ./scripts/generate-private-signing-identity.sh OUTPUT_DIRECTORY

Creates one stable, self-signed macOS code-signing identity for private
DuckGooKey distributions. The PKCS#12 password is read from
DUCKGOOKEY_PRIVATE_SIGNING_PASSWORD, or prompted for on an interactive terminal.

The output contains the encrypted P12, the public certificate, and fingerprints.
It never contains an unencrypted private key. Keep the output outside this repo.
EOF
}

die() {
  printf 'generate-private-signing-identity: %s\n' "$*" >&2
  exit 1
}

if (( $# != 1 )); then
  usage >&2
  exit 2
fi

[[ "$(uname -s)" == "Darwin" ]] || die "private macOS identities must be generated on macOS"
for command in openssl shasum; do
  command -v "$command" >/dev/null 2>&1 || die "$command is required"
done

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
project_dir="$(cd "$script_dir/.." && pwd -P)"
requested_output="$1"
output_parent="$(dirname "$requested_output")"
output_name="$(basename "$requested_output")"

[[ -n "$output_name" && "$output_name" != "." && "$output_name" != "/" ]] || \
  die "OUTPUT_DIRECTORY must name a new directory"
mkdir -p "$output_parent"
output_parent="$(cd "$output_parent" && pwd -P)"
output_dir="$output_parent/$output_name"

case "$output_dir/" in
  "$project_dir/"*)
    die "refusing to create signing keys inside the DuckGooKey repository"
    ;;
esac
[[ ! -e "$output_dir" ]] || die "output already exists: $output_dir"

if [[ -z "${DUCKGOOKEY_PRIVATE_SIGNING_PASSWORD:-}" ]]; then
  [[ -t 0 ]] || die "DUCKGOOKEY_PRIVATE_SIGNING_PASSWORD is required"
  read -r -s -p "Private signing password: " DUCKGOOKEY_PRIVATE_SIGNING_PASSWORD
  printf '\n'
  read -r -s -p "Confirm password: " password_confirmation
  printf '\n'
  [[ "$DUCKGOOKEY_PRIVATE_SIGNING_PASSWORD" == "$password_confirmation" ]] || \
    die "passwords do not match"
fi
(( ${#DUCKGOOKEY_PRIVATE_SIGNING_PASSWORD} >= 16 )) || \
  die "private signing password must contain at least 16 characters"
export DUCKGOOKEY_PRIVATE_SIGNING_PASSWORD

common_name="${DUCKGOOKEY_PRIVATE_SIGNING_NAME:-DuckGooKey Private Code Signing}"
[[ -n "$common_name" && "$common_name" != *"/"* ]] || \
  die "DUCKGOOKEY_PRIVATE_SIGNING_NAME must be non-empty and cannot contain '/'"

umask 077
work_dir="$(mktemp -d "$output_parent/.duckgookey-private-identity.XXXXXX")"
cleanup() {
  rm -rf "$work_dir"
}
trap cleanup EXIT

key_pem="$work_dir/DuckGooKey-Private-Code-Signing.key.pem"
cert_pem="$work_dir/DuckGooKey-Private-Code-Signing.cert.pem"
cert_der="$work_dir/DuckGooKey-Private-Code-Signing.cer"
cert_p12="$work_dir/DuckGooKey-Private-Code-Signing.p12"
metadata="$work_dir/signing-metadata.env"

openssl req \
  -x509 \
  -newkey rsa:3072 \
  -sha256 \
  -days 825 \
  -subj "/CN=$common_name/O=DuckGooKey" \
  -addext "basicConstraints=critical,CA:FALSE" \
  -addext "keyUsage=critical,digitalSignature" \
  -addext "extendedKeyUsage=critical,codeSigning" \
  -addext "subjectKeyIdentifier=hash" \
  -keyout "$key_pem" \
  -passout env:DUCKGOOKEY_PRIVATE_SIGNING_PASSWORD \
  -out "$cert_pem"

openssl pkcs12 \
  -export \
  -inkey "$key_pem" \
  -passin env:DUCKGOOKEY_PRIVATE_SIGNING_PASSWORD \
  -in "$cert_pem" \
  -name "$common_name" \
  -out "$cert_p12" \
  -passout env:DUCKGOOKEY_PRIVATE_SIGNING_PASSWORD
openssl x509 -in "$cert_pem" -outform DER -out "$cert_der"

certificate_sha256="$(shasum -a 256 "$cert_der" | awk '{print $1}')"
certificate_sha1="$(
  openssl x509 -in "$cert_pem" -noout -fingerprint -sha1 |
    awk -F= '{gsub(":", "", $2); print toupper($2)}'
)"
[[ "$certificate_sha256" =~ ^[0-9a-f]{64}$ ]] || die "failed to calculate certificate SHA-256"
[[ "$certificate_sha1" =~ ^[0-9A-F]{40}$ ]] || die "failed to calculate certificate SHA-1"

cat > "$metadata" <<EOF
DUCKGOOKEY_PRIVATE_SIGNING_CERT_SHA256=$certificate_sha256
DUCKGOOKEY_PRIVATE_SIGNING_CERT_SHA1=$certificate_sha1
EOF

rm -f "$key_pem"
chmod 600 "$cert_p12"
chmod 644 "$cert_pem" "$cert_der" "$metadata"
mv "$work_dir" "$output_dir"
trap - EXIT

printf 'Created a stable DuckGooKey private signing identity:\n'
printf '  Directory:          %s\n' "$output_dir"
printf '  Encrypted P12:      %s\n' "$output_dir/$(basename "$cert_p12")"
printf '  Public certificate: %s\n' "$output_dir/$(basename "$cert_der")"
printf '  Certificate SHA-256: %s\n' "$certificate_sha256"
printf '\nThis identity is for private distribution only. It is not an Apple\n'
printf 'Developer ID certificate and cannot be notarized by Apple.\n'
