#!/bin/bash
# ============================================================================
# load_concurrent_rw.sh — Concurrent read/write load test.
# ============================================================================
#
# Runs load_get.sh (100 readers) and load_post.sh (10 writers) simultaneously,
# waits for both to finish, then reports combined results.
#
# This simulates a realistic mixed workload where users are reading account
# data and reports while transactions are being posted concurrently.
#
# Usage:
#   ./load_concurrent_rw.sh
#
# Prerequisites: wrk, curl, jq, running API server.

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# ── Generate auth token once (shared by both child scripts) ─────────────────

if ! source "${SCRIPT_DIR}/generate_token.sh"; then
    echo "[load_concurrent_rw] ERROR: Failed to generate auth token." >&2
    exit 1
fi

# ── Check for wrk ───────────────────────────────────────────────────────────

if ! command -v wrk &>/dev/null; then
    echo "[load_concurrent_rw] ERROR: wrk is required." >&2
    echo "  Install:  apt install wrk  |  brew install wrk  |  https://github.com/wg/wrk" >&2
    exit 1
fi

# ── Parameters ──────────────────────────────────────────────────────────────

READER_CONNS=100
WRITER_CONNS=10
DURATION=60s

TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

echo ""
echo "============================================================"
echo "  Concurrent Read/Write Load Test"
echo "  Readers: ${READER_CONNS} (GET requests)"
echo "  Writers: ${WRITER_CONNS} (POST transactions)"
echo "  Duration: ${DURATION}"
echo "  Server:  ${API_BASE}"
echo "============================================================"
echo ""

# ── Start readers in background ─────────────────────────────────────────────
# load_get.sh sources generate_token.sh internally, but since $TOKEN is already
# exported, the source is a no-op and the token is reused.

echo "[load_concurrent_rw] Starting ${READER_CONNS} readers ..."
bash "${SCRIPT_DIR}/load_get.sh" 4 "${READER_CONNS}" "${DURATION}" \
    > "${TMPDIR}/readers.log" 2>&1 &
READER_PID=$!
echo "[load_concurrent_rw] Reader PID: ${READER_PID}"

# ── Start writers in background ─────────────────────────────────────────────
# load_post.sh accepts the connection count as its first argument.

echo "[load_concurrent_rw] Starting ${WRITER_CONNS} writers ..."
bash "${SCRIPT_DIR}/load_post.sh" "${WRITER_CONNS}" "${DURATION}" \
    > "${TMPDIR}/writers.log" 2>&1 &
WRITER_PID=$!
echo "[load_concurrent_rw] Writer PID: ${WRITER_PID}"

echo ""
echo "[load_concurrent_rw] Both tests running. Waiting for completion ..."
echo ""

# ── Wait for both to finish ─────────────────────────────────────────────────

READER_STATUS=0
WRITER_STATUS=0

wait "$READER_PID" || READER_STATUS=$?
wait "$WRITER_PID" || WRITER_STATUS=$?

# ── Report results ──────────────────────────────────────────────────────────

echo "============================================================"
echo "  Reader Results (GET — ${READER_CONNS} connections)"
echo "============================================================"
echo ""
cat "${TMPDIR}/readers.log"
echo ""

echo "============================================================"
echo "  Writer Results (POST — ${WRITER_CONNS} connections)"
echo "============================================================"
echo ""
cat "${TMPDIR}/writers.log"
echo ""

# ── Combined status ─────────────────────────────────────────────────────────

echo "============================================================"
echo "  Combined Status"
echo "============================================================"
echo -n "  Readers:  "
if [ "$READER_STATUS" -eq 0 ]; then echo "PASS"; else echo "FAIL (exit ${READER_STATUS})"; fi
echo -n "  Writers:  "
if [ "$WRITER_STATUS" -eq 0 ]; then echo "PASS"; else echo "FAIL (exit ${WRITER_STATUS})"; fi
echo -n "  Overall:  "
if [ "$READER_STATUS" -eq 0 ] && [ "$WRITER_STATUS" -eq 0 ]; then
    echo "PASS — concurrent read/write test completed successfully"
else
    echo "FAIL — one or both tests reported errors"
fi
echo ""
echo "============================================================"
