#!/bin/bash
# ============================================================================
# load_get.sh — Read-heavy GET load test with wrk.
# ============================================================================
#
# Tests three read endpoints under concurrent load:
#   1. GET /api/v1/accounts
#   2. GET /api/v1/transactions
#   3. GET /api/v1/reports/balance_sheet
#
# Default: 4 threads, 100 connections, 60 seconds per endpoint.
#
# Pass criteria:
#   - 0 errors (no Non-2xx/3xx responses)
#   - p99 latency < 500 ms
#
# Usage:
#   ./load_get.sh [threads] [connections] [duration]
#   ./load_get.sh                    # 4 threads, 100 conns, 60s
#   ./load_get.sh 8 200 30s          # 8 threads, 200 conns, 30s
#
# Prerequisites: wrk, curl, jq, running API server.

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# ── Generate auth token (sourced — exports $TOKEN and $API_BASE) ────────────

if ! source "${SCRIPT_DIR}/generate_token.sh"; then
    echo "[load_get] ERROR: Failed to generate auth token." >&2
    exit 1
fi

# ── Check for wrk ───────────────────────────────────────────────────────────

if ! command -v wrk &>/dev/null; then
    echo "[load_get] ERROR: wrk is required." >&2
    echo "  Install:  apt install wrk  |  brew install wrk  |  https://github.com/wg/wrk" >&2
    exit 1
fi

# ── Parameters ──────────────────────────────────────────────────────────────

THREADS="${1:-4}"
CONNECTIONS="${2:-100}"
DURATION="${3:-60s}"
AUTH_HEADER="Authorization: Bearer ${TOKEN}"

ENDPOINTS=(
    "/api/v1/accounts"
    "/api/v1/transactions"
    "/api/v1/reports/balance_sheet"
)

# ── Helper: extract p99 latency in milliseconds from wrk output ─────────────

extract_p99_ms() {
    local output="$1"
    local p99_raw
    p99_raw=$(echo "$output" | grep '^\s*99%' | awk '{print $2}')

    if [ -z "$p99_raw" ]; then
        echo "-1"
        return
    fi

    # wrk reports latencies with units: us, ms, s
    if [[ "$p99_raw" == *us ]]; then
        # microseconds → milliseconds
        echo "${p99_raw%us}" | awk '{printf "%.0f", $1 / 1000}'
    elif [[ "$p99_raw" == *ms ]]; then
        echo "${p99_raw%ms}" | awk '{printf "%.0f", $1}'
    elif [[ "$p99_raw" == *s ]]; then
        # seconds → milliseconds
        echo "${p99_raw%s}" | awk '{printf "%.0f", $1 * 1000}'
    else
        echo "$p99_raw"
    fi
}

# ── Helper: extract error count from wrk output ─────────────────────────────

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

# ── Header ──────────────────────────────────────────────────────────────────

echo ""
echo "============================================================"
echo "  GET Load Test"
echo "  Threads: ${THREADS}   Connections: ${CONNECTIONS}   Duration: ${DURATION}"
echo "  Server:  ${API_BASE}"
echo "============================================================"
echo ""

# ── Run each endpoint ───────────────────────────────────────────────────────

for endpoint in "${ENDPOINTS[@]}"; do
    url="${API_BASE}${endpoint}"
    echo "--- GET ${endpoint} ---"
    echo "    URL: ${url}"
    echo ""

    OUTPUT=$(wrk -t"${THREADS}" -c"${CONNECTIONS}" -d"${DURATION}" \
        -H "${AUTH_HEADER}" \
        "${url}" 2>&1) || true

    echo "$OUTPUT"
    echo ""

    # ── Evaluate pass criteria ──────────────────────────────────────────
    P99=$(extract_p99_ms "$OUTPUT")
    ERRORS=$(extract_errors "$OUTPUT")

    echo "    p99 latency:   ${P99} ms"
    echo "    errors:        ${ERRORS}"
    echo -n "    Status:        "

    if [ "$ERRORS" -eq 0 ] && [ "$P99" -ge 0 ] && [ "$P99" -lt 500 ]; then
        echo "PASS"
    else
        echo "FAIL (criteria: 0 errors, p99 < 500ms)"
    fi
    echo ""
done

echo "============================================================"
echo "  GET Load Test Complete"
echo "============================================================"
