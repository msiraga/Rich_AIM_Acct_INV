#Requires -Version 5.1
<#
.SYNOPSIS
    NexusLedger audit script — Windows PowerShell equivalent of audit.sh.

.DESCRIPTION
    Runs the full quality gate:
      1. cargo test --all
      2. cargo audit
      3. cargo clippy --all -- -D warnings
      4. cargo fmt --all --check
      5. cargo deny check          (non-fatal: warns if unavailable)
      6. npm audit --audit-level=moderate

    The script resolves the repository root from its own location, so it can be
    invoked from any working directory.  A non-zero exit code is returned as
    soon as a fatal check fails.

.NOTES
    cargo-audit and cargo-deny are not shipped with Rust and must be installed
    separately:
        cargo install cargo-audit
        cargo install cargo-deny
#>
Set-StrictMode -Version Latest

$RepoRoot = Split-Path -Parent $PSScriptRoot

function Write-Step { param([string]$Message) Write-Host $Message -ForegroundColor Cyan }
function Write-Ok   { param([string]$Message) Write-Host "OK $Message" -ForegroundColor Green }
function Write-Fail { param([string]$Message) Write-Host "FAILED: $Message" -ForegroundColor Red }

Write-Host "=== NexusLedger Audit ===" -ForegroundColor White
Write-Host ""

# 1. cargo test --all
Write-Step "1. Running cargo test --all..."
Push-Location "$RepoRoot\RichdaleAccounting"
try {
    cargo test --all 2>&1 | Out-Host
    if ($LASTEXITCODE -ne 0) { Write-Fail "cargo test (exit $LASTEXITCODE)"; exit 1 }
} finally { Pop-Location }
Write-Ok "Tests passed"
Write-Host ""

# 2. cargo audit
Write-Step "2. Running cargo audit..."
Push-Location "$RepoRoot\RichdaleAccounting"
try {
    cargo audit 2>&1 | Out-Host
    if ($LASTEXITCODE -ne 0) {
        Write-Host "Install cargo-audit: cargo install cargo-audit" -ForegroundColor Yellow
        Write-Fail "cargo audit (exit $LASTEXITCODE)"
        exit 1
    }
} finally { Pop-Location }
Write-Ok "No vulnerabilities"
Write-Host ""

# 3. cargo clippy
Write-Step "3. Running cargo clippy..."
Push-Location "$RepoRoot\RichdaleAccounting"
try {
    cargo clippy --all -- -D warnings 2>&1 | Out-Host
    if ($LASTEXITCODE -ne 0) { Write-Fail "cargo clippy (exit $LASTEXITCODE)"; exit 1 }
} finally { Pop-Location }
Write-Ok "Clippy clean"
Write-Host ""

# 4. cargo fmt --check
Write-Step "4. Running cargo fmt check..."
Push-Location "$RepoRoot\RichdaleAccounting"
try {
    cargo fmt --all --check 2>&1 | Out-Host
    if ($LASTEXITCODE -ne 0) { Write-Fail "cargo fmt (exit $LASTEXITCODE)"; exit 1 }
} finally { Pop-Location }
Write-Ok "Formatting clean"
Write-Host ""

# 5. cargo deny check (non-fatal: warns if cargo-deny is missing or reports issues)
Write-Step "5. Running cargo deny..."
Push-Location "$RepoRoot\RichdaleAccounting"
try {
    cargo deny check 2>&1 | Out-Host
    if ($LASTEXITCODE -ne 0) {
        Write-Host "Install cargo-deny: cargo install cargo-deny" -ForegroundColor Yellow
        Write-Host "cargo deny reported issues or is unavailable — continuing" -ForegroundColor Yellow
    } else {
        Write-Ok "Licenses and advisories OK"
    }
} finally { Pop-Location }
Write-Host ""

# 6. npm audit
Write-Step "6. Running npm audit..."
Push-Location "$RepoRoot\nexus-ledger-tauri"
try {
    npm audit --audit-level=moderate 2>&1 | Out-Host
    if ($LASTEXITCODE -ne 0) { Write-Fail "npm audit (exit $LASTEXITCODE)"; exit 1 }
} finally { Pop-Location }
Write-Ok "Frontend deps OK"
Write-Host ""

Write-Host "=== ALL CHECKS PASSED ===" -ForegroundColor Green
exit 0
