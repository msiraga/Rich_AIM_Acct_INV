#!/bin/bash
# ─────────────────────────────────────────────────────────────────────────────
# hey load test for NexusLedger API
#
# Tests each endpoint with hey (https://github.com/rakyll/hey), a modern
# HTTP load testing tool. hey is simpler than wrk but provides good
# latency distribution output.
#
# Prerequisites:
#   - hey installed (go install github.com/rakyll/hey@latest or download
#     a binary release from https://github.com/rakyll/hey/releases)
#   - NexusLedger API running on http://localhost:8080
#   - curl installed (for auth token setup)
#   - jq installed (for JSON parsing)
#
# Usage:
#   ./hey_bench.sh [duration] [concurrent]
#
#   duration   - test duration per endpoint in seconds (default: 30)
#   concurrent - number of concurrent workers (default: 100)
#
# Examples:
#   ./hey_bench.sh                # 30s, 100 workers
#   ./hey_bench.sh 60 200         # 60s, 200 workers
#   ./hey_bench.sh 10 50          # quick smoke test
# ─────────────────────────────────────────────────────────────────────────────

set -euo pipefail

API_BASE="${API_BASE:-http://localhost:8080}"
DURATION="${1:-30}"
CONCURRENT="${2:-100}"

# hey uses -q (rate limit) and -n (number of requests) or -z (duration)
# We use -z for duration-based testing.
# Total requests per endpoint ≈ CONCURRENT * DURATION * (requests/sec per worker)

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

echo -e "${CYAN}"
echo "═══════════════════════════════════════════════════════════════"
echo "  NexusLedger hey Load Test"
echo "  Duration     : ${DURATION}s"
echo "  Concurrent   : ${CONCURRENT}"
echo "  API base URL : ${API_BASE}"
echo "═══════════════════════════════════════════════════════════════"
echo -e "${NC}"

# ── Prerequisite checks ─────────────────────────────────────────────────────

check_cmd() {
    if ! command -v "$1" &>/dev/null; then
        echo -e "${RED}Error: '$1' is not installed. Please install it first.${NC}"
        if [ "$1" = "hey" ]; then
            echo "  Install hey with:"
            echo "    go install github.com/rakyll/hey@latest"
            echo "  Or download a binary from:"
            echo "    https://github.com/rakyll/hey/releases"
        fi
        exit 1
    fi
}

check_cmd hey
check_cmd curl
check_cmd jq

# ── Auth setup ──────────────────────────────────────────────────────────────

echo -e "${YELLOW}[1/3] Setting up authentication...${NC}"

UNIQUE="heybench_$(date +%s)"
USERNAME="heybench_${UNIQUE}"
EMAIL="heybench_${UNIQUE}@nexusledger.test"
PASSWORD="HeyBench123!"

# Register a test user
REGISTER_RESP=$(curl -s -X POST "${API_BASE}/api/auth/register" \
    -H "Content-Type: application/json" \
    -d "{\"username\":\"${USERNAME}\",\"email\":\"${EMAIL}\",\"password\":\"${PASSWORD}\"}")

# Try to extract the token from registration; fall back to login
TOKEN=$(echo "${REGISTER_RESP}" | jq -r '.data.access_token // empty')

if [ -z "${TOKEN}" ]; then
    echo "  Registration returned no token, attempting login..."
    LOGIN_RESP=$(curl -s -X POST "${API_BASE}/api/auth/login" \
        -H "Content-Type: application/json" \
        -d "{\"username\":\"${USERNAME}\",\"password\":\"${PASSWORD}\"}")
    TOKEN=$(echo "${LOGIN_RESP}" | jq -r '.data.access_token // empty')
fi

if [ -z "${TOKEN}" ]; then
    echo -e "${RED}Error: Failed to obtain JWT token. Is the API running?${NC}"
    echo "  Register response: ${REGISTER_RESP}"
    exit 1
fi

echo -e "  ${GREEN}✓ JWT token acquired${NC}"

# Fetch an account ID for POST transaction tests
ACCOUNT_ID=$(curl -s "${API_BASE}/api/v1/accounts" \
    -H "Authorization: Bearer ${TOKEN}" | jq -r '.data[0].id // empty')

if [ -z "${ACCOUNT_ID}" ]; then
    echo -e "  ${YELLOW}⚠ No account IDs found — POST transaction tests will be skipped${NC}"
else
    echo -e "  ${GREEN}✓ Account ID for POST tests: ${ACCOUNT_ID}${NC}"
fi

# ── Prepare POST body ───────────────────────────────────────────────────────

echo -e "${YELLOW}[2/3] Preparing test payloads...${NC}"

# hey reads the POST body from a file with -d / -D
POST_BODY_FILE=$(mktemp /tmp/nexus_hey_body_XXXXXX.json)
trap "rm -f '${POST_BODY_FILE}'" EXIT

if [ -n "${ACCOUNT_ID}" ]; then
    cat > "${POST_BODY_FILE}" << JSONEOF
{
    "description": "hey bench transaction",
    "entries": [
        {"account_id": "${ACCOUNT_ID}", "amount": "10.00", "entry_type": "debit", "description": "hey debit"},
        {"account_id": "${ACCOUNT_ID}", "amount": "10.00", "entry_type": "credit", "description": "hey credit"}
    ]
}
JSONEOF
    echo -e "  ${GREEN}✓ POST body prepared${NC}"
else
    echo -e "  ${YELLOW}⚠ POST body not prepared (no account ID)${NC}"
fi

echo -e "${YELLOW}[3/3] Running benchmarks...${NC}"
echo ""

RESULTS_FILE="hey_results_$(date +%Y%m%d_%H%M%S).txt"
echo "NexusLedger hey Load Test Results" > "${RESULTS_FILE}"
echo "Date: $(date)" >> "${RESULTS_FILE}"
echo "Duration: ${DURATION}s  Concurrent: ${CONCURRENT}" >> "${RESULTS_FILE}"
echo "" >> "${RESULTS_FILE}"

# ── Benchmark runner ───────────────────────────────────────────────────────

run_bench() {
    local label="$1"
    local url="$2"
    local auth_header="$3"
    local method="${4:-GET}"
    local extra_flags=""

    echo -e "${CYAN}── ${label} (${method}) ──────────────────────────────────${NC}"
    echo ""

    if [ "${method}" = "POST" ]; then
        if [ ! -s "${POST_BODY_FILE}" ]; then
            echo -e "${YELLOW}Skipping (no POST body available)${NC}"
            echo ""
            return
        fi
        extra_flags="-m POST -T application/json -D ${POST_BODY_FILE}"
    fi

    local output
    output=$(hey -z "${DURATION}s" -c "${CONCURRENT}" \
        -H "${auth_header}" \
        ${extra_flags} \
        "${url}" 2>&1 || true)

    echo "${output}"
    echo ""

    echo "── ${label} (${method}) ──" >> "${RESULTS_FILE}"
    echo "${output}" >> "${RESULTS_FILE}"
    echo "" >> "${RESULTS_FILE}"
}

# Auth header for authenticated endpoints
AUTH_HEADER="Authorization: Bearer ${TOKEN}"

# Public endpoint (no auth)
run_bench "GET /health"              "${API_BASE}/health"              ""                 "GET"

# Authenticated GET endpoints
run_bench "GET /api/v1/status"       "${API_BASE}/api/v1/status"       "${AUTH_HEADER}"   "GET"
run_bench "GET /api/v1/accounts"     "${API_BASE}/api/v1/accounts"     "${AUTH_HEADER}"   "GET"
run_bench "GET /api/v1/transactions" "${API_BASE}/api/v1/transactions" "${AUTH_HEADER}"   "GET"
run_bench "GET /api/v1/agents"       "${API_BASE}/api/v1/agents"       "${AUTH_HEADER}"   "GET"
run_bench "GET /api/v1/tasks/queue"  "${API_BASE}/api/v1/tasks/queue"  "${AUTH_HEADER}"   "GET"

# Authenticated POST endpoint
run_bench "POST /api/v1/transactions" "${API_BASE}/api/v1/transactions" "${AUTH_HEADER}"   "POST"

# ── Summary ─────────────────────────────────────────────────────────────────

echo -e "${CYAN}═══════════════════════════════════════════════════════════════${NC}"
echo -e "${GREEN}  Benchmark complete. Results saved to: ${RESULTS_FILE}${NC}"
echo -e "${CYAN}═══════════════════════════════════════════════════════════════${NC}"
echo ""
echo "Pass criteria:"
echo "  - GET endpoints: p99 latency < 500ms, 0 errors"
echo "  - POST endpoints: p99 latency < 1000ms, 0 errors"
echo ""
echo "hey output key metrics:"
echo "  - Requests/sec: throughput"
echo "  - Latency distribution: p50, p75, p90, p95, p99 (in ms)"
echo "  - Status code distribution: should be all 200/201"
echo ""
