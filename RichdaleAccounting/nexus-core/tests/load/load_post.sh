#!/bin/bash
# ============================================================================
# load_post.sh — Write-heavy POST load test with wrk + Lua.
# ============================================================================
#
# Uses wrk with post_script.lua to POST balanced double-entry transactions
# (Dr Cash 100 / Cr Revenue 100) to /api/v1/transactions.
#
# Default: 4 threads, 50 connections, 60 seconds.
#
# Pass criteria:
#   - 0 errors (no Non-2xx/3xx responses)
#   - p99 latency < 1000 ms (1 s)
#
# Usage:
#   ./load_post.sh [connections] [duration]
#   ./load_post.sh             # 50 conns, 60s (default)
#   ./load_post.sh 10          # 10 conns, 60s (used by concurrent_rw)
#   ./load_post.sh 25 30s     # 25 conns, 30s
#
# Prerequisites: wrk, curl, jq, running API server with initialised chart
# of accounts (account 1000 = Cash, account 4000 = Sales Revenue).

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# ── Generate auth token (sourced — exports $TOKEN and $API_BASE) ────────────

if ! source "${SCRIPT_DIR}/generate_token.sh"; then
    echo "[load_post] ERROR: Failed to generate auth token." >&2
    exit 1
fi

# ── Check for wrk ───────────────────────────────────────────────────────────

if ! command -v wrk &>/dev/null; then
    echo "[load_post] ERROR: wrk is required." >&2
    echo "  Install:  apt install wrk  |  brew install wrk  |  https://github.com/wg/wrk" >&2
    exit 1
fi

# ── Parameters ──────────────────────────────────────────────────────────────

THREADS=4
CONNECTIONS="${1:-50}"
DURATION="${2:-60s}"
LUA_SCRIPT="${SCRIPT_DIR}/post_script.lua"
AUTH_HEADER="Authorization: Bearer ${TOKEN}"
CONTENT_TYPE_HEADER="Content-Type: application/json"
POST_URL="${API_BASE}/api/v1/transactions"

# ── Fetch account UUIDs from the running server ────────────────────────────
# Account 1000 = Cash (Asset), Account 4000 = Sales Revenue (Revenue).
# The POST /api/v1/transactions endpoint requires valid account UUIDs.

echo "[load_post] Fetching account UUIDs from ${API_BASE}/api/v1/accounts ..."

ACCOUNTS_RESPONSE=$(curl -s \
    -H "${AUTH_HEADER}" \
    "${API_BASE}/api/v1/accounts" 2>/dev/null)

if [ -z "$ACCOUNTS_RESPONSE" ]; then
    echo "[load_post] ERROR: No response from /api/v1/accounts." >&2
    echo "  Is the API server running at ${API_BASE}?" >&2
    exit 1
fi

CASH_ID=$(echo "$ACCOUNTS_RESPONSE" | jq -r '.data[] | select(.number == "1000") | .id' | head -1)
REVENUE_ID=$(echo "$ACCOUNTS_RESPONSE" | jq -r '.data[] | select(.number == "4000") | .id' | head -1)

if [ -z "$CASH_ID" ] || [ "$CASH_ID" = "null" ]; then
    echo "[load_post] ERROR: Could not find Cash account (number 1000)." >&2
    echo "  Response: ${ACCOUNTS_RESPONSE}" >&2
    exit 1
fi

if [ -z "$REVENUE_ID" ] || [ "$REVENUE_ID" = "null" ]; then
    echo "[load_post] ERROR: Could not find Sales Revenue account (number 4000)." >&2
    echo "  Response: ${ACCOUNTS_RESPONSE}" >&2
    exit 1
fi

echo "[load_post] Cash account ID:     ${CASH_ID}"
echo "[load_post] Revenue account ID:  ${REVENUE_ID}"
echo ""

# ── Helper: extract p99 latency in milliseconds ─────────────────────────────

extract_p99_ms() {
    local output="$1"
    local p99_raw
    p99_raw=$(echo "$output" | grep '^\s*99%' | awk '{print $2}')

    if [ -z "$p99_raw" ]; then
        echo "-1"
        return
    fi

    if [[ "$p99_raw" == *us ]]; then
        echo "${p99_raw%us}" | awk '{printf "%.0f", $1 / 1000}'
    elif [[ "$p99_raw" == *ms ]]; then
        echo "${p99_raw%ms}" | awk '{printf "%.0f", $1}'
    elif [[ "$p99_raw" == *s ]]; then
        echo "${p99_raw%s}" | awk '{printf "%.0f", $1 * 1000}'
    else
        echo "$p99_raw"
    fi
}

# ── Helper: extract error count ─────────────────────────────────────────────

extract_errors() {
    local output="$1"
    local err_line
    err_line=$(echo "$output" | grep "Non-2xx or 3xx responses:" || true)
    if [ -z "$err_line" ]; then
        echo "0"
    else
        echo "$err_line" | grep -o '[0-9]\+' | tail -1
    fi
}

# ── Run wrk with Lua script ─────────────────────────────────────────────────

echo "============================================================"
echo "  POST Load Test (write transactions)"
echo "  Threads: ${THREADS}   Connections: ${CONNECTIONS}   Duration: ${DURATION}"
echo "  URL:     ${POST_URL}"
echo "  Body:    Dr Cash 100 / Cr Revenue 100 (balanced double-entry)"
echo "============================================================"
echo ""

# Account UUIDs are passed as Lua script arguments after "--".
# The Authorization and Content-Type headers are set via -H flags.
# wrk.format("POST", nil, nil, body) in the Lua script inherits wrk.headers
# (which includes the -H flags) and the URL from the command line.

OUTPUT=$(wrk -t"${THREADS}" -c"${CONNECTIONS}" -d"${DURATION}" \
    -s "${LUA_SCRIPT}" \
    -H "${AUTH_HEADER}" \
    -H "${CONTENT_TYPE_HEADER}" \
    "${POST_URL}" \
    -- "${CASH_ID}" "${REVENUE_ID}" 2>&1) || true

echo "$OUTPUT"
echo ""

# ── Evaluate pass criteria ──────────────────────────────────────────────────

P99=$(extract_p99_ms "$OUTPUT")
ERRORS=$(extract_errors "$OUTPUT")

echo "    p99 latency:   ${P99} ms"
echo "    errors:        ${ERRORS}"
echo -n "    Status:        "

if [ "$ERRORS" -eq 0 ] && [ "$P99" -ge 0 ] && [ "$P99" -lt 1000 ]; then
    echo "PASS"
else
    echo "FAIL (criteria: 0 errors, p99 < 1000ms)"
fi

echo ""
echo "============================================================"
echo "  POST Load Test Complete"
echo "============================================================"
