#!/usr/bin/env bash
set -euo pipefail
set +x

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=analytics-keychain.sh
source "$script_dir/analytics-keychain.sh"

period="${1:-weekly}"
case "$period" in
  daily) interval="1 DAY" ;;
  weekly) interval="7 DAY" ;;
  monthly) interval="30 DAY" ;;
  *)
    printf 'usage: analytics-report.sh [daily|weekly|monthly]\n' >&2
    exit 2
    ;;
esac

command -v security >/dev/null 2>&1 || { printf 'analytics-report: macOS security CLI is required\n' >&2; exit 1; }
command -v curl >/dev/null 2>&1 || { printf 'analytics-report: curl is required\n' >&2; exit 1; }
command -v jq >/dev/null 2>&1 || { printf 'analytics-report: jq is required; run mise install --locked\n' >&2; exit 1; }

account_id="$(duckgookey_analytics_read_item "$DUCKGOOKEY_ANALYTICS_ACCOUNT_ID")" || { printf 'analytics-report: Cloudflare Account ID is unavailable; run mise run analytics-configure\n' >&2; exit 1; }
token="$(duckgookey_analytics_read_item "$DUCKGOOKEY_ANALYTICS_API_TOKEN")" || { printf 'analytics-report: Cloudflare API token is unavailable; run mise run analytics-configure\n' >&2; exit 1; }

query() {
  curl --fail --silent --show-error \
    "https://api.cloudflare.com/client/v4/accounts/$account_id/analytics_engine/sql" \
    --header "Authorization: Bearer $token" \
    --data-binary "$1"
}

event_query="SELECT blob1 AS event, blob2 AS platform, blob3 AS version, SUM(_sample_interval) AS count FROM duckgookey_events WHERE timestamp > NOW() - INTERVAL '$interval' GROUP BY event, platform, version ORDER BY count DESC FORMAT JSON"
active_query="SELECT COUNT(DISTINCT index1) AS active_installations FROM duckgookey_events WHERE timestamp > NOW() - INTERVAL '$interval' AND blob1 = 'active_daily' FORMAT JSON"

printf 'DuckGooKey anonymous usage report · %s\n\n' "$period"
printf 'Event totals\n'
query "$event_query" | jq .
printf '\nActive installations\n'
query "$active_query" | jq .
