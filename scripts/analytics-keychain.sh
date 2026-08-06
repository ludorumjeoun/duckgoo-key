#!/usr/bin/env bash

DUCKGOOKEY_ANALYTICS_KEYCHAIN_SERVICE="com.duckgoo.key.analytics"
DUCKGOOKEY_ANALYTICS_ACCOUNT_ID="CLOUDFLARE_ACCOUNT_ID"
DUCKGOOKEY_ANALYTICS_API_TOKEN="CLOUDFLARE_ANALYTICS_API_TOKEN"

duckgookey_analytics_keychain() {
  local keychain
  keychain="$(security default-keychain -d user 2>/dev/null)" || return 1
  keychain="$(printf '%s' "$keychain" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')"
  keychain="${keychain#\"}"
  keychain="${keychain%\"}"
  [[ -n "$keychain" ]] || return 1
  printf '%s' "$keychain"
}

duckgookey_analytics_read_item() {
  local account="$1"
  local keychain
  keychain="$(duckgookey_analytics_keychain)" || return 1
  security find-generic-password -a "$account" -s "$DUCKGOOKEY_ANALYTICS_KEYCHAIN_SERVICE" -w "$keychain" 2>/dev/null
}
