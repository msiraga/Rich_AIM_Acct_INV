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

# Deliberately NOT stopping on first error: we want all checks to run and
# produce a full report. $ErrorActionPreference stays at its default of
# 'Continue' so native-command failures do not abort the script.

$ErrorActionPreference = 'Continue'

# cd to the project root (parent of the scripts/ directory).
$ProjectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $ProjectRoot

# ─── State ────────────────────────────────────────────────────────────
$script:Results = @()

# ─── Helpers ──────────────────────────────────────────────────────────
function Invoke-Check {
    param(
        [string]   $Label,
        [scriptblock]$Command
    )
    Write-Host ""
    Write-Host "> $Label" -ForegroundColor Cyan

    $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    $outputLines = @()
    try {
        $outputLines = & $Command 2>&1
    } catch {
        $outputLines = "$_"
    }
    $code = $LASTEXITCODE
    if ($null -eq $code) { $code = 0 }
    $stopwatch.Stop()
    $secs = [int][math]::Round($stopwatch.Elapsed.TotalSeconds)

    # Normalize captured objects (stderr arrives as ErrorRecords in PS 5.1)
    # into plain text lines.
    $output = ($outputLines | ForEach-Object { "$_" }) -join "`n"

    if ($code -eq 0) {
        Write-Host "  [PASS] passed ($secs s)" -ForegroundColor Green
    } else {
        Write-Host "  [FAIL] failed ($secs s)" -ForegroundColor Red
        Write-Host "  -- last 20 lines of output:" -ForegroundColor DarkGray
        $tail = $output -split "`n"
        $start = [math]::Max(0, $tail.Count - 20)
        $tail[$start..($tail.Count - 1)] | ForEach-Object {
            Write-Host "    $_" -ForegroundColor Red
        }
    }

    $script:Results += [pscustomobject]@{
        Label   = $Label
        Passed  = ($code -eq 0)
        Seconds = $secs
    }
}

# ─── Preflight ───────────────────────────────────────────────────────
Write-Host "==================================================" -ForegroundColor White
Write-Host "       NexusLedger Audit -- Quality Gate          " -ForegroundColor White
Write-Host "==================================================" -ForegroundColor White
Write-Host "Working directory: $ProjectRoot" -ForegroundColor DarkGray
Write-Host ""

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Host "[FAIL] Rust/Cargo not found. Install from https://rustup.rs" -ForegroundColor Red
    exit 1
}
Write-Host "[ OK ] Cargo: $(cargo --version)" -ForegroundColor Green

# Ensure cargo-audit is available; install if missing.
$auditInstalled = $true
& cargo audit --version 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) {
    $auditInstalled = $false
    Write-Host "[WARN] cargo-audit not installed -- installing..." -ForegroundColor Yellow
    & cargo install cargo-audit 2>&1 | Out-Null
    if ($LASTEXITCODE -eq 0) {
        $auditInstalled = $true
        Write-Host "[ OK ] cargo-audit installed" -ForegroundColor Green
    } else {
        Write-Host "[FAIL] Could not install cargo-audit." -ForegroundColor Red
        Write-Host "       The security check below will fail until it is installed manually:" -ForegroundColor DarkGray
        Write-Host "         cargo install cargo-audit" -ForegroundColor DarkGray
    }
}
Write-Host ""

# ─── Checks ──────────────────────────────────────────────────────────
Invoke-Check "Formatting (cargo fmt --check)"         { cargo fmt --check }
Invoke-Check "Linting (cargo clippy -- -D warnings)"  { cargo clippy --all -- -D warnings }
Invoke-Check "Tests (cargo test --all)"               { cargo test --all }
Invoke-Check "Security (cargo audit)"                 { cargo audit }
Invoke-Check "Release build (cargo build --release)"  { cargo build --release }

# ─── Summary ─────────────────────────────────────────────────────────
$total  = $script:Results.Count
$passed = ($script:Results | Where-Object { $_.Passed }).Count
$failed = $total - $passed

Write-Host ""
Write-Host "==================================================" -ForegroundColor White
Write-Host "  AUDIT SUMMARY" -ForegroundColor White
Write-Host "==================================================" -ForegroundColor White
foreach ($r in $script:Results) {
    if ($r.Passed) {
        $marker = "OK"
        $color  = 'Green'
    } else {
        $marker = "FAIL"
        $color  = 'Red'
    }
    $name = $r.Label.PadRight(42)
    Write-Host -NoNewline "  "
    Write-Host -NoNewline $marker -ForegroundColor $color
    Write-Host (" {0} {1}s" -f $name, $r.Seconds)
}
Write-Host "--------------------------------------------------" -ForegroundColor DarkGray

if ($failed -eq 0) {
    Write-Host "  All $total checks passed. NexusLedger is ready to ship." -ForegroundColor Green
    exit 0
} else {
    Write-Host "  $failed of $total checks failed. Resolve the issues above before tagging a release." -ForegroundColor Red
    exit 1
}
