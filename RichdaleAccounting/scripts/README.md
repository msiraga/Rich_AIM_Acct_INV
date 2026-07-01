# NexusLedger Audit Scripts

A single command that verifies everything passes before a release tag.
This is the **final quality gate** for Phase 7 (Task 7.15).

## What It Does

The audit runs five checks, **all of them**, every time — it never stops on
the first failure. A summary table is printed at the end.

| # | Check | Command | What it guarantees |
|---|-------|---------|--------------------|
| 1 | Formatting | `cargo fmt --check` | Source code is `rustfmt`-clean |
| 2 | Linting | `cargo clippy --all -- -D warnings` | Zero Clippy warnings (warnings = errors) |
| 3 | Tests | `cargo test --all` | All unit + integration tests pass |
| 4 | Security | `cargo audit` | Zero known CVEs in the dependency tree |
| 5 | Release build | `cargo build --release` | The release binary compiles successfully |

Each step is timed. If a step fails, the last 20 lines of its output are
shown inline so you can see what broke without re-running anything.

## Prerequisites

- **Rust toolchain** (`cargo`, `rustc`, `rustfmt`, `clippy`) — install from
  <https://rustup.rs>. The `rustfmt` and `clippy` components come with the
  default `rustup` profile.
- **`cargo-audit`** — if not installed, the script installs it automatically
  (`cargo install cargo-audit`). An internet connection is required for the
  one-time install and for fetching the advisory database.
- The audit operates on the **RichdaleAccounting** Rust workspace (the
  `nexus-core` crate). Run it from a checkout that has been built at least
  once so a `Cargo.lock` exists (required by `cargo audit`).

## Usage

### macOS / Linux / Git Bash

```bash
./RichdaleAccounting/scripts/audit.sh
```

### Windows (PowerShell)

```powershell
.\RichdaleAccounting\scripts\audit.ps1
```

Both scripts `cd` into the project root automatically, so they can be
invoked from any directory.

## Reading the Output

```
╔════════════════════════════════════════════════╗
║       NexusLedger Audit — Quality Gate        ║
╚════════════════════════════════════════════════╝
Working directory: /path/to/RichdaleAccounting

▶ Formatting (cargo fmt --check)
  ✓ passed (1s)
...
═══════════════════════════════════════════════════════════
  AUDIT SUMMARY
═══════════════════════════════════════════════════════════
  ✓ Formatting (cargo fmt --check)                     1s
  ✗ Linting (cargo clippy -- -D warnings)             12s
  ✓ Tests (cargo test --all)                          45s
  ✓ Security (cargo audit)                             8s
  ✓ Release build (cargo build --release)            120s
───────────────────────────────────────────────────────────
  1 of 5 checks failed. Resolve the issues above before tagging a release.
```

Green `✓` = passed, red `✗` = failed. On Windows PowerShell the same
information is shown with `OK` / `FAIL` markers and `Write-Host` colors.

## Exit Codes

| Code | Meaning |
|------|---------|
| `0`  | All checks passed — safe to tag a release |
| `1`  | One or more checks failed — do not release |

This makes the script suitable for wiring into CI: the pipeline fails iff
the audit fails.

## Notes

- The scripts intentionally do **not** use `set -e` / `$ErrorActionPreference = 'Stop'`.
  Every check runs to completion so you get the full picture in one pass.
- `cargo audit` checks the advisory database from
  <https://github.com/rustsec/advisory-db>. Run it periodically even outside
  of releases to catch newly-disclosed vulnerabilities.
- The Tauri frontend (`nexus-ledger-tauri/`) is a separate workspace with its
  own Node/Cargo build (`npm run build` / `cargo tauri build`) and is not
  covered by this Rust-focused audit.
