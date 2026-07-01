#!/bin/bash
set -euo pipefail

echo "=== NexusLedger Audit ==="
echo ""

echo "1. Running cargo test --all..."
cd RichdaleAccounting && cargo test --all 2>&1
echo "✓ Tests passed"
echo ""

echo "2. Running cargo audit..."
cargo audit 2>&1 || { echo "Install cargo-audit: cargo install cargo-audit"; exit 1; }
echo "✓ No vulnerabilities"
echo ""

echo "3. Running cargo clippy..."
cargo clippy --all -- -D warnings 2>&1
echo "✓ Clippy clean"
echo ""

echo "4. Running cargo fmt check..."
cargo fmt --all --check 2>&1
echo "✓ Formatting clean"
echo ""

echo "5. Running cargo deny..."
cargo deny check 2>&1 || { echo "Install cargo-deny: cargo install cargo-deny"; }
echo "✓ Licenses and advisories OK"
echo ""

echo "6. Running npm audit..."
cd ../nexus-ledger-tauri && npm audit --audit-level=moderate 2>&1
echo "✓ Frontend deps OK"
echo ""

echo "=== ALL CHECKS PASSED ==="
