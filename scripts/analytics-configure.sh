#!/usr/bin/env bash
set -euo pipefail
set +x
umask 077

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=analytics-keychain.sh
source "$script_dir/analytics-keychain.sh"

[[ "$(uname -s)" == "Darwin" ]] || { printf 'analytics-configure: macOS Keychain is required\n' >&2; exit 1; }
command -v security >/dev/null 2>&1 || { printf 'analytics-configure: macOS security CLI is required\n' >&2; exit 1; }
command -v curl >/dev/null 2>&1 || { printf 'analytics-configure: curl is required\n' >&2; exit 1; }

keychain="$(duckgookey_analytics_keychain)" || { printf 'analytics-configure: could not resolve the default user Keychain\n' >&2; exit 1; }

account_id=""
printf 'Cloudflare Account ID: ' >&2
IFS= read -r account_id || { printf 'analytics-configure: input ended before account ID was provided\n' >&2; exit 1; }
[[ "$account_id" =~ ^[a-fA-F0-9]{32}$ ]] || { printf 'analytics-configure: account ID must be 32 hexadecimal characters\n' >&2; exit 1; }

security add-generic-password \
  -U \
  -a "$DUCKGOOKEY_ANALYTICS_ACCOUNT_ID" \
  -s "$DUCKGOOKEY_ANALYTICS_KEYCHAIN_SERVICE" \
  -D "DuckGooKey analytics configuration" \
  -l "DuckGooKey Cloudflare Account ID" \
  -j "Managed by mise run analytics-configure" \
  -w "$account_id" >/dev/null

printf 'Enter a Cloudflare API token with Account Analytics:Read in the Keychain prompt.\n' >&2
security add-generic-password \
  -U \
  -a "$DUCKGOOKEY_ANALYTICS_API_TOKEN" \
  -s "$DUCKGOOKEY_ANALYTICS_KEYCHAIN_SERVICE" \
  -D "DuckGooKey analytics credential" \
  -l "DuckGooKey Cloudflare Analytics API token" \
  -j "Managed by mise run analytics-configure" \
  -w >/dev/null

token="$(duckgookey_analytics_read_item "$DUCKGOOKEY_ANALYTICS_API_TOKEN")" || { printf 'analytics-configure: Keychain token could not be read\n' >&2; exit 1; }
response="$(curl --fail --silent --show-error \
  "https://api.cloudflare.com/client/v4/accounts/$account_id/analytics_engine/sql" \
  --header "Authorization: Bearer $token" \
  --data "SELECT 'DuckGooKey analytics ready' AS status")" || { printf 'analytics-configure: Cloudflare token validation failed\n' >&2; exit 1; }

printf '%s' "$response" | grep -q 'DuckGooKey analytics ready' || { printf 'analytics-configure: Cloudflare returned an unexpected response\n' >&2; exit 1; }
printf 'Anonymous analytics reporting is ready. Run: mise run analytics-report -- weekly\n'
