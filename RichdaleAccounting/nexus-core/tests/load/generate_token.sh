#!/bin/bash
# ============================================================================
# generate_token.sh — Register a test user and export a JWT access token.
# ============================================================================
#
# Must be SOURCED by other scripts so the exported $TOKEN is visible:
#
#   source "$(dirname "$0")/generate_token.sh"
#
# Exports:
#   TOKEN      — JWT access token (Bearer)
#   API_BASE   — Base URL of the API server (default http://localhost:4000)
#   TEST_USER  — Username of the registered test user
#   TEST_EMAIL — Email of the registered test user
#
# If $TOKEN is already set the script is a no-op, so it is safe to source
# multiple times.
#
# Prerequisites: curl, jq, and a running NexusLedger API server.

# ── Configuration ───────────────────────────────────────────────────────────

API_BASE="${API_BASE:-http://localhost:4000}"
REGISTER_ENDPOINT="${API_BASE}/api/auth/register"

# ── Skip if token already exists ────────────────────────────────────────────

if [ -n "${TOKEN:-}" ]; then
    echo "[generate_token] TOKEN already set — skipping registration."
    return 0 2>/dev/null || exit 0
fi

# ── Dependency checks ───────────────────────────────────────────────────────

if ! command -v curl &>/dev/null; then
    echo "[generate_token] ERROR: curl is required." >&2
    return 1 2>/dev/null || exit 1
fi

if ! command -v jq &>/dev/null; then
    echo "[generate_token] ERROR: jq is required." >&2
    echo "  Install:  apt install jq  |  brew install jq  |  choco install jq" >&2
    return 1 2>/dev/null || exit 1
fi

# ── Generate unique test credentials ────────────────────────────────────────
# A random suffix ensures the script can be run repeatedly without
# "username already taken" errors.

SUFFIX=$(date +%s%N 2>/dev/null | tail -c 7 || echo "$RANDOM")
TEST_USER="loadtest_${SUFFIX}"
TEST_EMAIL="loadtest_${SUFFIX}@nexusledger.test"
TEST_PASSWORD="LoadTest123!"   # 12 chars, has letters + digits — passes strength check

echo "[generate_token] Registering test user: ${TEST_USER}"

# ── POST /api/auth/register ─────────────────────────────────────────────────
# Response shape:
#   {
#     "success": true,
#     "data": {
#       "user_id": "...",
#       "username": "...",
#       "role": "user",
#       "access_token": "eyJ...",
#       "refresh_token": "eyJ...",
#       "expires_in": 1800
#     },
#     "metadata": { ... }
#   }

HTTP_RESPONSE=$(curl -s -w "\n%{http_code}" \
    -X POST "${REGISTER_ENDPOINT}" \
    -H "Content-Type: application/json" \
    -d "{\"username\":\"${TEST_USER}\",\"email\":\"${TEST_EMAIL}\",\"password\":\"${TEST_PASSWORD}\"}" \
    2>/dev/null)

HTTP_CODE=$(echo "$HTTP_RESPONSE" | tail -1)
RESPONSE_BODY=$(echo "$HTTP_RESPONSE" | sed '$d')

if [ "$HTTP_CODE" != "201" ]; then
    echo "[generate_token] ERROR: Registration failed (HTTP ${HTTP_CODE})" >&2
    echo "[generate_token] Response: ${RESPONSE_BODY}" >&2
    return 1 2>/dev/null || exit 1
fi

# ── Extract access_token ────────────────────────────────────────────────────

TOKEN=$(echo "$RESPONSE_BODY" | jq -r '.data.access_token')

if [ -z "$TOKEN" ] || [ "$TOKEN" = "null" ]; then
    echo "[generate_token] ERROR: Failed to extract access_token from response." >&2
    echo "[generate_token] Response: ${RESPONSE_BODY}" >&2
    return 1 2>/dev/null || exit 1
fi

export TOKEN API_BASE TEST_USER TEST_EMAIL

echo "[generate_token] Token generated successfully."
echo "[generate_token] User: ${TEST_USER}"
echo "[generate_token] Token (first 40 chars): ${TOKEN:0:40}..."
