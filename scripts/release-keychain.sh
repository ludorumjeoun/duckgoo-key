#!/usr/bin/env bash

# Shared macOS Keychain contract for local Cloudflare R2 publication.
# This file is sourced by release-configure.sh and release-local.sh.

DUCKGOOKEY_R2_KEYCHAIN_SERVICE="com.duckgoo.key.release.r2"
DUCKGOOKEY_R2_ACCESS_KEY_ACCOUNT="CLOUDFLARE_R2_ACCESS_KEY_ID"
DUCKGOOKEY_R2_SECRET_KEY_ACCOUNT="CLOUDFLARE_R2_SECRET_ACCESS_KEY"
DUCKGOOKEY_R2_KEYCHAIN_KIND="DuckGooKey local release credential"
DUCKGOOKEY_R2_KEYCHAIN_COMMENT="Managed by mise run release-configure"

DUCKGOOKEY_DEFAULT_USER_KEYCHAIN=""
DUCKGOOKEY_RESOLVED_R2_ACCESS_KEY_ID=""
DUCKGOOKEY_RESOLVED_R2_SECRET_ACCESS_KEY=""
DUCKGOOKEY_R2_CREDENTIAL_ERROR=""

duckgookey_resolve_default_user_keychain() {
  local keychain=""

  DUCKGOOKEY_R2_CREDENTIAL_ERROR=""
  command -v security >/dev/null 2>&1 || {
    DUCKGOOKEY_R2_CREDENTIAL_ERROR="macOS security CLI is required"
    return 1
  }
  if ! keychain="$(security default-keychain -d user 2>/dev/null)"; then
    DUCKGOOKEY_R2_CREDENTIAL_ERROR="could not resolve the default user Keychain"
    return 1
  fi
  keychain="${keychain#\"}"
  keychain="${keychain%\"}"
  if [[ -z "$keychain" ]]; then
    DUCKGOOKEY_R2_CREDENTIAL_ERROR="the default user Keychain path is empty"
    return 1
  fi

  DUCKGOOKEY_DEFAULT_USER_KEYCHAIN="$keychain"
}

duckgookey_r2_keychain_item_exists() {
  local account="$1"

  if [[ -z "$DUCKGOOKEY_DEFAULT_USER_KEYCHAIN" ]]; then
    duckgookey_resolve_default_user_keychain || return 1
  fi
  security find-generic-password \
    -a "$account" \
    -s "$DUCKGOOKEY_R2_KEYCHAIN_SERVICE" \
    "$DUCKGOOKEY_DEFAULT_USER_KEYCHAIN" >/dev/null 2>&1
}

duckgookey_store_r2_keychain_item() {
  local account="$1"
  local label="$2"

  # macOS security intentionally prompts without echo when -w is the final
  # argument. Supplying the value after -w would expose it in process argv.
  security add-generic-password \
    -U \
    -a "$account" \
    -s "$DUCKGOOKEY_R2_KEYCHAIN_SERVICE" \
    -D "$DUCKGOOKEY_R2_KEYCHAIN_KIND" \
    -l "$label" \
    -j "$DUCKGOOKEY_R2_KEYCHAIN_COMMENT" \
    -w
}

duckgookey_delete_r2_keychain_item() {
  local account="$1"

  [[ -n "$DUCKGOOKEY_DEFAULT_USER_KEYCHAIN" ]] || return 1
  security delete-generic-password \
    -a "$account" \
    -s "$DUCKGOOKEY_R2_KEYCHAIN_SERVICE" \
    "$DUCKGOOKEY_DEFAULT_USER_KEYCHAIN" >/dev/null 2>&1
}

duckgookey_read_r2_keychain_credentials() {
  local access_key_id=""
  local secret_access_key=""

  DUCKGOOKEY_R2_CREDENTIAL_ERROR=""
  DUCKGOOKEY_RESOLVED_R2_ACCESS_KEY_ID=""
  DUCKGOOKEY_RESOLVED_R2_SECRET_ACCESS_KEY=""

  if [[ -z "$DUCKGOOKEY_DEFAULT_USER_KEYCHAIN" ]]; then
    duckgookey_resolve_default_user_keychain || return 1
  fi
  if ! access_key_id="$(
    security find-generic-password \
      -a "$DUCKGOOKEY_R2_ACCESS_KEY_ACCOUNT" \
      -s "$DUCKGOOKEY_R2_KEYCHAIN_SERVICE" \
      -w \
      "$DUCKGOOKEY_DEFAULT_USER_KEYCHAIN" 2>/dev/null
  )"; then
    DUCKGOOKEY_R2_CREDENTIAL_ERROR="Keychain item $DUCKGOOKEY_R2_ACCESS_KEY_ACCOUNT is unavailable; run mise run release-configure"
    return 1
  fi
  if [[ -z "$access_key_id" ]]; then
    DUCKGOOKEY_R2_CREDENTIAL_ERROR="Keychain item $DUCKGOOKEY_R2_ACCESS_KEY_ACCOUNT is empty; run mise run release-configure"
    return 1
  fi
  if ! secret_access_key="$(
    security find-generic-password \
      -a "$DUCKGOOKEY_R2_SECRET_KEY_ACCOUNT" \
      -s "$DUCKGOOKEY_R2_KEYCHAIN_SERVICE" \
      -w \
      "$DUCKGOOKEY_DEFAULT_USER_KEYCHAIN" 2>/dev/null
  )"; then
    DUCKGOOKEY_R2_CREDENTIAL_ERROR="Keychain item $DUCKGOOKEY_R2_SECRET_KEY_ACCOUNT is unavailable; run mise run release-configure"
    return 1
  fi
  if [[ -z "$secret_access_key" ]]; then
    DUCKGOOKEY_R2_CREDENTIAL_ERROR="Keychain item $DUCKGOOKEY_R2_SECRET_KEY_ACCOUNT is empty; run mise run release-configure"
    return 1
  fi

  DUCKGOOKEY_RESOLVED_R2_ACCESS_KEY_ID="$access_key_id"
  DUCKGOOKEY_RESOLVED_R2_SECRET_ACCESS_KEY="$secret_access_key"
}

duckgookey_resolve_r2_credentials_from_values() {
  local env_access_key_id="$1"
  local env_secret_access_key="$2"

  DUCKGOOKEY_R2_CREDENTIAL_ERROR=""
  DUCKGOOKEY_RESOLVED_R2_ACCESS_KEY_ID=""
  DUCKGOOKEY_RESOLVED_R2_SECRET_ACCESS_KEY=""

  if [[ -n "$env_access_key_id" || -n "$env_secret_access_key" ]]; then
    if [[ -z "$env_access_key_id" || -z "$env_secret_access_key" ]]; then
      DUCKGOOKEY_R2_CREDENTIAL_ERROR="R2 environment override is incomplete; set both CLOUDFLARE_R2_ACCESS_KEY_ID and CLOUDFLARE_R2_SECRET_ACCESS_KEY, or unset both to use Keychain"
      return 1
    fi
    DUCKGOOKEY_RESOLVED_R2_ACCESS_KEY_ID="$env_access_key_id"
    DUCKGOOKEY_RESOLVED_R2_SECRET_ACCESS_KEY="$env_secret_access_key"
    return 0
  fi

  duckgookey_read_r2_keychain_credentials
}

duckgookey_resolve_r2_credentials() {
  duckgookey_resolve_r2_credentials_from_values \
    "${CLOUDFLARE_R2_ACCESS_KEY_ID:-}" \
    "${CLOUDFLARE_R2_SECRET_ACCESS_KEY:-}"
}
