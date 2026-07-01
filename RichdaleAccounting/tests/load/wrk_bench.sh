#!/bin/bash
# ─────────────────────────────────────────────────────────────────────────────
# wrk load test for NexusLedger API
#
# Tests each endpoint with wrk, using a Lua script to inject JWT auth headers.
#
# Prerequisites:
#   - wrk installed (https://github.com/wg/wrk)
#   - NexusLedger API running on http://localhost:8080
#   - curl installed (for auth token setup)
#   - jq installed (for JSON parsing)
#
# Usage:
#   ./wrk_bench.sh [duration] [connections]
#
#   duration     - test duration per endpoint (default: 30s)
#   connections  - number of concurrent connections (default: 100)
#
# Examples:
#   ./wrk_bench.sh                # 30s, 100 connections
#   ./wrk_bench.sh 60s 200        # 60s, 200 connections
#   ./wrk_bench.sh 10s 50         # quick smoke test
# ─────────────────────────────────────────────────────────────────────────────

set -euo pipefail

API_BASE="${API_BASE:-http://localhost:8080}"
DURATION="${1:-30s}"
CONNECTIONS="${2:-100}"
THREADS="${THREADS:-$(nproc 2>/dev/null || echo 4)}"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

echo -e "${CYAN}"
echo "═══════════════════════════════════════════════════════════════"
echo "  NexusLedger wrk Load Test"
echo "  Duration     : ${DURATION}"
echo "  Connections  : ${CONNECTIONS}"
echo "  Threads      : ${THREADS}"
echo "  API base URL : ${API_BASE}"
echo "═══════════════════════════════════════════════════════════════"
echo -e "${NC}"

# ── Prerequisite checks ─────────────────────────────────────────────────────

check_cmd() {
    if ! command -v "$1" &>/dev/null; then
        echo -e "${RED}Error: '$1' is not installed. Please install it first.${NC}"
        exit 1
    fi
}

check_cmd wrk
check_cmd curl
check_cmd jq

# ── Auth setup ──────────────────────────────────────────────────────────────

echo -e "${YELLOW}[1/3] Setting up authentication...${NC}"

UNIQUE="wrkbench_$(date +%s)"
USERNAME="wrkbench_${UNIQUE}"
EMAIL="wrkbench_${UNIQUE}@nexusledger.test"
PASSWORD="WrkBench123!"

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

# ── Generate wrk Lua auth script ────────────────────────────────────────────

echo -e "${YELLOW}[2/3] Generating wrk Lua auth script...${NC}"

LUA_SCRIPT=$(mktemp /tmp/nexus_wrk_auth_XXXXXX.lua)
trap "rm -f '${LUA_SCRIPT}'" EXIT

cat > "${LUA_SCRIPT}" << LUAEOF
-- wrk auth header injection script
-- Adds the JWT Bearer token to every request

local auth_header = "Authorization: Bearer ${TOKEN}"
local account_id = "${ACCOUNT_ID}"

wrk.headers["Authorization"] = auth_header

-- For POST requests, set content type and body
local txn_body = string.format(
    '{"description":"wrk bench transaction","entries":[
        {"account_id":"%s","amount":"10.00","entry_type":"debit","description":"wrk debit"},
        {"account_id":"%s","amount":"10.00","entry_type":"credit","description":"wrk credit"}
    ]}',
    account_id, account_id
)

request = function()
    if wrk.method == "POST" then
        wrk.headers["Content-Type"] = "application/json"
        return wrk.format(nil, nil, nil, txn_body)
    end
    return wrk.format(nil)
end
LUAEOF

# Also generate a simple Lua script for unauthenticated (health) endpoint
LUA_SCRIPT_HEALTH=$(mktemp /tmp/nexus_wrk_health_XXXXXX.lua)
trap "rm -f '${LUA_SCRIPT}' '${LUA_SCRIPT_HEALTH}'" EXIT

cat > "${LUA_SCRIPT_HEALTH}" << 'LUAEOF'
-- No auth needed for /health
request = function()
    return wrk.format(nil)
end
LUAEOF

echo -e "  ${GREEN}✓ Lua scripts generated${NC}"

# ── Run benchmarks ──────────────────────────────────────────────────────────

echo -e "${YELLOW}[3/3] Running benchmarks...${NC}"
echo ""

RESULTS_FILE="wrk_results_$(date +%Y%m%d_%H%M%S).txt"
echo "NexusLedger wrk Load Test Results" > "${RESULTS_FILE}"
echo "Date: $(date)" >> "${RESULTS_FILE}"
echo "Duration: ${DURATION}  Connections: ${CONNECTIONS}  Threads: ${THREADS}" >> "${RESULTS_FILE}"
echo "" >> "${RESULTS_FILE}"

run_bench() {
    local label="$1"
    local url="$2"
    local use_auth="$3"
    local method="${4:-GET}"

    echo -e "${CYAN}── ${label} (${method}) ──────────────────────────────────${NC}"
    echo ""

    local script=""
    if [ "${use_auth}" = "yes" ]; then
        script="-s ${LUA_SCRIPT}"
    else
        script="-s ${LUA_SCRIPT_HEALTH}"
    fi

    # Set method via Lua script for POST
    if [ "${method}" = "POST" ]; then
        # Override the Lua script to use POST method
        local post_script=$(mktemp /tmp/nexus_wrk_post_XXXXXX.lua)
        cat > "${post_script}" << POSTLUA
wrk.method = "POST"
wrk.headers["Authorization"] = "Bearer ${TOKEN}"
wrk.headers["Content-Type"] = "application/json"
local account_id = "${ACCOUNT_ID}"
local txn_body = string.format(
    '{"description":"wrk bench transaction","entries":[{"account_id":"%s","amount":"10.00","entry_type":"debit","description":"wrk debit"},{"account_id":"%s","amount":"10.00","entry_type":"credit","description":"wrk credit"}]}',
    account_id, account_id
)
request = function()
    return wrk.format(nil, nil, nil, txn_body)
end
POSTLUA
        script="-s ${post_script}"
        trap "rm -f '${LUA_SCRIPT}' '${LUA_SCRIPT_HEALTH}' '${post_script}'" EXIT
    fi

    local output
    output=$(wrk -t"${THREADS}" -c"${CONNECTIONS}" -d"${DURATION}" ${script} "${url}" 2>&1 || true)

    echo "${output}"
    echo ""

    echo "── ${label} (${method}) ──" >> "${RESULTS_FILE}"
    echo "${output}" >> "${RESULTS_FILE}"
    echo "" >> "${RESULTS_FILE}"
}

# Public endpoint (no auth)
run_bench "GET /health"              "${API_BASE}/health"              "no"

# Authenticated GET endpoints
run_bench "GET /api/v1/status"       "${API_BASE}/api/v1/status"       "yes"
run_bench "GET /api/v1/accounts"     "${API_BASE}/api/v1/accounts"     "yes"
run_bench "GET /api/v1/transactions" "${API_BASE}/api/v1/transactions" "yes"
run_bench "GET /api/v1/agents"       "${API_BASE}/api/v1/agents"       "yes"
run_bench "GET /api/v1/tasks/queue"  "${API_BASE}/api/v1/tasks/queue"  "yes"

# Authenticated POST endpoint
if [ -n "${ACCOUNT_ID}" ]; then
    run_bench "POST /api/v1/transactions" "${API_BASE}/api/v1/transactions" "yes" "POST"
else
    echo -e "${YELLOW}Skipping POST /api/v1/transactions (no account ID available)${NC}"
    echo ""
fi

# ── Summary ─────────────────────────────────────────────────────────────────

echo -e "${CYAN}═══════════════════════════════════════════════════════════════${NC}"
echo -e "${GREEN}  Benchmark complete. Results saved to: ${RESULTS_FILE}${NC}"
echo -e "${CYAN}═══════════════════════════════════════════════════════════════${NC}"
echo ""
echo "Pass criteria:"
echo "  - GET endpoints: p99 latency < 500ms, 0 errors"
echo "  - POST endpoints: p99 latency < 1000ms, 0 errors"
echo ""
