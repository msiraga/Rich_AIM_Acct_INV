#!/bin/bash

# NexusLedger Audit Script — Final Quality Gate (Phase 7, Task 7.15)
#
# Runs every check that must be green before a release tag:
#   1. cargo fmt --check                 — formatting is clean
#   2. cargo clippy --all -- -D warnings — zero lint warnings
#   3. cargo test --all                  — all unit + integration tests pass
#   4. cargo audit                       — zero known CVEs in dependencies
#   5. cargo build --release             — release binary builds successfully
#
# Every check runs to completion regardless of earlier failures, then a
# summary table is printed. Exits 0 only if all checks passed; 1 otherwise.
#
# Author: Mounir Siraji <mounir@richdaleai.com>
# License: Apache-2.0

# Deliberately NOT `set -e`: we want all checks to run and produce a full
# report, not abort on the first failure.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.." || { echo "FATAL: cannot cd to project root"; exit 1; }

# ─── Colors ───────────────────────────────────────────────────────────
if [ -t 1 ]; then
    GREEN='\033[0;32m'
    RED='\033[0;31m'
    YELLOW='\033[0;33m'
    BLUE='\033[0;34m'
    BOLD='\033[1m'
    DIM='\033[2m'
    RESET='\033[0m'
else
    GREEN='' RED='' YELLOW='' BLUE='' BOLD='' DIM='' RESET=''
fi

# ─── State ────────────────────────────────────────────────────────────
declare -a CHECK_NAMES
declare -a CHECK_RESULTS
declare -a CHECK_TIMES
WORK_DIR=$(mktemp -d)
trap 'rm -rf "$WORK_DIR"' EXIT

# ─── Helpers ──────────────────────────────────────────────────────────
info() { printf "${BLUE}▶${RESET} ${BOLD}%s${RESET}\n" "$1"; }

run_check() {
    local label="$1"
    local logfile="$WORK_DIR/${#CHECK_NAMES[@]}.log"
    shift
    info "$label"
    SECONDS=0
    "$@" >"$logfile" 2>&1
    local rc=$?
    local secs=$SECONDS
    CHECK_NAMES+=("$label")
    CHECK_TIMES+=("$secs")
    if [ $rc -eq 0 ]; then
        CHECK_RESULTS+=("PASS")
        printf "  ${GREEN}✓ passed${RESET} ${DIM}(%ds)${RESET}\n\n" "$secs"
    else
        CHECK_RESULTS+=("FAIL")
        printf "  ${RED}✗ failed${RESET} ${DIM}(%ds)${RESET}\n" "$secs"
        printf "  ${DIM}— last 20 lines of output:${RESET}\n"
        tail -n 20 "$logfile" | sed 's/^/    /'
        printf "\n"
    fi
}

# ─── Preflight ───────────────────────────────────────────────────────
printf "${BOLD}╔════════════════════════════════════════════════╗${RESET}\n"
printf "${BOLD}║       NexusLedger Audit — Quality Gate        ║${RESET}\n"
printf "${BOLD}╚════════════════════════════════════════════════╝${RESET}\n"
printf "${DIM}Working directory: %s${RESET}\n\n" "$(pwd)"

if ! command -v cargo >/dev/null 2>&1; then
    printf "${RED}✗ Rust/Cargo not found. Install from https://rustup.rs${RESET}\n"
    exit 1
fi
printf "${GREEN}✓${RESET} Cargo: %s\n" "$(cargo --version)"

# Ensure cargo-audit is available; install if missing.
if ! cargo audit --version >/dev/null 2>&1; then
    printf "${YELLOW}⚠ cargo-audit not installed — installing...${RESET}\n"
    if cargo install cargo-audit >"$WORK_DIR/install.log" 2>&1; then
        printf "${GREEN}✓${RESET} cargo-audit installed\n"
    else
        printf "${RED}✗ Could not install cargo-audit (see $WORK_DIR/install.log)${RESET}\n"
        printf "${DIM}  The security check below will fail until it is installed manually:${RESET}\n"
        printf "${DIM}    cargo install cargo-audit${RESET}\n"
    fi
fi
printf "\n"

# ─── Checks ──────────────────────────────────────────────────────────
run_check "Formatting (cargo fmt --check)"         cargo fmt --check
run_check "Linting (cargo clippy -- -D warnings)"  cargo clippy --all -- -D warnings
run_check "Tests (cargo test --all)"               cargo test --all
run_check "Security (cargo audit)"                 cargo audit
run_check "Release build (cargo build --release)"  cargo build --release

# ─── Summary ─────────────────────────────────────────────────────────
total=${#CHECK_NAMES[@]}
passed=0
for r in "${CHECK_RESULTS[@]}"; do
    [ "$r" = "PASS" ] && passed=$((passed + 1))
done
failed=$((total - passed))

printf "${BOLD}═══════════════════════════════════════════════════════════${RESET}\n"
printf "${BOLD}  AUDIT SUMMARY${RESET}\n"
printf "${BOLD}═══════════════════════════════════════════════════════════${RESET}\n"
for ((i = 0; i < total; i++)); do
    name="${CHECK_NAMES[$i]}"
    res="${CHECK_RESULTS[$i]}"
    secs="${CHECK_TIMES[$i]}"
    if [ "$res" = "PASS" ]; then
        marker="${GREEN}✓${RESET}"
    else
        marker="${RED}✗${RESET}"
    fi
    printf "  %b %-42s %3ds\n" "$marker" "$name" "$secs"
done
printf "${DIM}───────────────────────────────────────────────────────────${RESET}\n"

if [ "$failed" -eq 0 ]; then
    printf "  ${GREEN}${BOLD}All %d checks passed.${RESET} NexusLedger is ready to ship.\n" "$total"
    exit 0
else
    printf "  ${RED}${BOLD}%d of %d checks failed.${RESET} Resolve the issues above before tagging a release.\n" "$failed" "$total"
    exit 1
fi
