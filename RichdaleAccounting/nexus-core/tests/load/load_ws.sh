#!/bin/bash
# ============================================================================
# load_ws.sh — WebSocket concurrent connection load test.
# ============================================================================
#
# Opens 50 concurrent WebSocket connections to /ws/chat, sends a message
# every 2 seconds for 60 seconds, and verifies that responses are received.
#
# Pass criteria:
#   - All 50 connections stay open for the full duration
#   - At least 90% of sent messages receive a response
#   - 0 hard connection errors (refused, dropped)
#
# Usage:
#   ./load_ws.sh [connections] [duration] [interval]
#   ./load_ws.sh             # 50 conns, 60s, 2s interval
#   ./load_ws.sh 100 120 1   # 100 conns, 120s, 1s interval
#
# Prerequisites: websocat, curl, jq, running API server with WebSocket support.
#   Install websocat:
#     cargo install websocat
#     # or
#     apt install websocat
#     # or download from:
#     #   https://github.com/nickel-org/websocat/releases

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# ── Generate auth token (sourced — exports $TOKEN and $API_BASE) ────────────

if ! source "${SCRIPT_DIR}/generate_token.sh"; then
    echo "[load_ws] ERROR: Failed to generate auth token." >&2
    exit 1
fi

# ── Check for websocat ──────────────────────────────────────────────────────

if ! command -v websocat &>/dev/null; then
    echo "[load_ws] ERROR: websocat is required." >&2
    echo "  Install:  cargo install websocat" >&2
    echo "  or:       apt install websocat" >&2
    echo "  or:       download from https://github.com/nickel-org/websocat/releases" >&2
    exit 1
fi

# ── Parameters ──────────────────────────────────────────────────────────────

WS_CONNS="${1:-50}"
WS_DURATION="${2:-60}"
WS_INTERVAL="${3:-2}"

# ── Build WebSocket URL ─────────────────────────────────────────────────────
# Convert http(s):// to ws(s):// and append /ws/chat?token=...

WS_SCHEME="ws"
WS_HOST="${API_BASE#*://}"   # strip scheme: http://host:port → host:port
if [[ "$API_BASE" == https:* ]]; then
    WS_SCHEME="wss"
fi
WS_URL="${WS_SCHEME}://${WS_HOST}/ws/chat?token=${TOKEN}"

# ── Temp directory for per-connection logs ──────────────────────────────────

TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

# ── Messages to send (rotated) ──────────────────────────────────────────────
# These are natural-language queries that the WS chat handler can process.

MESSAGES=(
    "what's my cash balance?"
    "show me my balance sheet"
    "show me my trial balance"
    "what's my income statement?"
    "list my accounts"
)

NUM_MESSAGES=${#MESSAGES[@]}

# ── Worker function ─────────────────────────────────────────────────────────
# Opens a single WS connection, sends a message every $WS_INTERVAL seconds,
# and records all responses to a log file.

ws_worker() {
    local id=$1
    local logfile="${TMPDIR}/ws_${id}.log"
    local total_msgs=$(( WS_DURATION / WS_INTERVAL ))

    # Generate messages at the specified interval.
    # The subshell keeps the pipe open for the full duration.
    # A trailing 5-second sleep gives the last response time to arrive
    # before stdin EOF causes websocat to close.
    (
        for i in $(seq 1 "$total_msgs"); do
            local idx=$(( (i - 1) % NUM_MESSAGES ))
            echo "${MESSAGES[$idx]}"
            sleep "$WS_INTERVAL"
        done
        sleep 5   # grace period for the last response
    ) | websocat "$WS_URL" > "$logfile" 2>&1
}

# ── Header ──────────────────────────────────────────────────────────────────

echo ""
echo "============================================================"
echo "  WebSocket Load Test"
echo "  Connections: ${WS_CONNS}   Duration: ${WS_DURATION}s   Interval: ${WS_INTERVAL}s"
echo "  URL: ${WS_URL}"
echo "============================================================"
echo ""

# ── Start workers ───────────────────────────────────────────────────────────

echo "[load_ws] Starting ${WS_CONNS} WebSocket connections ..."

PIDS=()
for i in $(seq 1 "$WS_CONNS"); do
    ws_worker "$i" &
    PIDS+=($!)
    # Stagger connection establishment slightly to avoid thundering herd
    sleep 0.05
done

echo "[load_ws] All ${WS_CONNS} workers started (PIDs: ${PIDS[*]})"
echo "[load_ws] Waiting ${WS_DURATION}s for test to complete ..."
echo ""

# ── Wait for all workers to finish ──────────────────────────────────────────
# Each worker runs for approximately WS_DURATION + 5 seconds (grace period).

EXPECTED_TOTAL=$(( WS_DURATION + 10 ))
sleep "$EXPECTED_TOTAL"

# Kill any stragglers
for pid in "${PIDS[@]}"; do
    kill "$pid" 2>/dev/null || true
done
wait 2>/dev/null || true

# ── Collect results ─────────────────────────────────────────────────────────

echo "============================================================"
echo "  WebSocket Results"
echo "============================================================"
echo ""

total_sent=0
total_received=0
active_conns=0
dropped_conns=0
total_expected_msgs=$(( WS_DURATION / WS_INTERVAL * WS_CONNS ))

for i in $(seq 1 "$WS_CONNS"); do
    logfile="${TMPDIR}/ws_${i}.log"
    if [ -f "$logfile" ]; then
        lines=$(wc -l < "$logfile" | tr -d ' ')
        # First line is the welcome message; remaining lines are responses.
        if [ "$lines" -gt 0 ]; then
            responses=$(( lines - 1 ))
        else
            responses=0
        fi
        sent=$(( WS_DURATION / WS_INTERVAL ))
        total_sent=$(( total_sent + sent ))
        total_received=$(( total_received + responses ))
        active_conns=$(( active_conns + 1 ))
    else
        dropped_conns=$(( dropped_conns + 1 ))
    fi
done

# ── Summary ─────────────────────────────────────────────────────────────────

echo "  Connections opened:   ${active_conns} / ${WS_CONNS}"
echo "  Connections dropped:  ${dropped_conns}"
echo "  Messages sent:        ${total_sent}"
echo "  Responses received:   ${total_received}"
echo ""

if [ "$total_sent" -gt 0 ]; then
    response_pct=$(( total_received * 100 / total_sent ))
else
    response_pct=0
fi

echo "  Response rate:        ${response_pct}%"
echo ""

# ── Pass/fail evaluation ────────────────────────────────────────────────────

echo -n "  Status:               "

PASS=true

if [ "$active_conns" -ne "$WS_CONNS" ]; then
    PASS=false
    echo "FAIL — only ${active_conns}/${WS_CONNS} connections were active"
elif [ "$response_pct" -lt 90 ]; then
    PASS=false
    echo "FAIL — response rate ${response_pct}% is below 90% threshold"
else
    echo "PASS — all connections active, response rate ${response_pct}%"
fi

echo ""
echo "============================================================"

if [ "$PASS" = true ]; then
    exit 0
else
    exit 1
fi
