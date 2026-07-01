# Building NexusLedger

This document covers building the NexusLedger desktop application for Windows and macOS.

## Prerequisites (Common)

- **Rust** (stable toolchain) — https://rustup.rs
- **Node.js** 18+ and npm — https://nodejs.org
- **Tauri CLI** — install with `cargo install tauri-cli` or use `npm run tauri` via the project's dev dependency

## Prerequisites — Windows

- Windows 10/11 (64-bit)
- **WebView2 Runtime** — pre-installed on Windows 11; on Windows 10 download from https://developer.microsoft.com/microsoft-edge/webview2/
- **Microsoft C++ Build Tools** (MSVC) — install "Desktop development with C++" workload from Visual Studio Build Tools
- **WiX Toolset** (optional, for MSI) — Tauri bundles a WiX binary; no separate install needed for recent Tauri v2 versions

## Prerequisites — macOS

- macOS 10.15 (Catalina) or later
- **Xcode Command Line Tools** — `xcode-select --install`
- For native ARM64 builds: an Apple Silicon Mac (M1/M2/M3+)
- For universal binaries (x86_64 + aarch64): install both targets:
  ```sh
  rustup target add x86_64-apple-darwin
  rustup target add aarch64-apple-darwin
  ```

## Building

From the `nexus-ledger-tauri` directory:

```sh
# Install frontend dependencies (first time only)
npm install

# Production build — produces installers for the current platform
cargo tauri build
# or equivalently:
npm run tauri build
```

### Windows Output

After a successful build, installers are in:

```
src-tauri/target/release/bundle/
├── msi/      ← NexusLedger_1.0.0_x64_en-US.msi
└── nsis/     ← NexusLedger_1.0.0_x64-setup.exe
```

The NSIS installer is configured for per-machine installation (`installMode: "perMachine"`).

### macOS Output

After a successful build, installers are in:

```
src-tauri/target/release/bundle/
├── dmg/      ← NexusLedger_1.0.0_aarch64.dmg (or x86_64)
└── macos/    ← NexusLedger.app
```

The DMG contains the `.app` bundle with a drag-to-Applications installer.

To build a universal binary (both Intel and Apple Silicon):

```sh
rustup target add aarch64-apple-darwin
rustup target add x86_64-apple-darwin
npm run tauri build -- --target universal-apple-darwin
```

## Code Signing (Optional)

### macOS Code Signing

1. Obtain a Developer ID Application certificate from the Apple Developer Program.
2. Export the signing identity name (e.g., "Developer ID Application: Your Name (TEAMID)").
3. Set the signing identity in `tauri.conf.json` under `bundle.macOS.signingIdentity`, or pass it via environment variable:

```sh
# Set the identity at build time
export APPLE_SIGNING_IDENTITY="Developer ID Application: Your Name (TEAMID)"
cargo tauri build
```

4. The `entitlements.plist` file in `src-tauri/` defines the app's entitlements. To activate it, set the `entitlements` path in `tauri.conf.json`:

```json
"macOS": {
    "signingIdentity": "Developer ID Application: Your Name (TEAMID)",
    "entitlements": "entitlements.plist"
}
```

5. For notarization (required for distribution outside the App Store):

```sh
export APPLE_ID="your-apple-id@example.com"
export APPLE_PASSWORD="app-specific-password"
export APPLE_TEAM_ID="TEAMID"
cargo tauri build
```

Tauri automatically submits the signed app for notarization and staples the ticket when these environment variables are set.

### Windows Code Signing

1. Obtain a code signing certificate (EV or OV) from a trusted CA.
2. Set environment variables before building:

```sh
# PowerShell
$env:TAURI_SIGNING_PRIVATE_KEY="path\to\key.pem"
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD="your-password"
cargo tauri build
```

This signs both the MSI and NSIS installers and enables automatic updates via the updater plugin.

## Verifying a Build

```sh
# Check the produced binaries
ls src-tauri/target/release/bundle/

# Verify macOS app signature (if signed)
codesign -dv --verbose=4 src-tauri/target/release/bundle/macos/NexusLedger.app

# Verify notarization ticket (if notarized)
xcrun stapler verify src-tauri/target/release/bundle/macos/NexusLedger.app
```

## Troubleshooting

- **"linker `link.exe` not found" (Windows):** Install MSVC Build Tools and run from a "Developer PowerShell" or run `rustup default stable-x86_64-pc-windows-msvc`.
- **"xcrun: error: invalid path" (macOS):** Run `xcode-select --install` to install Command Line Tools.
- **Icons missing:** Ensure icon files exist under `src-tauri/icons/`. Tauri requires PNG (32x32, 128x128, 128x128@2x), ICNS (macOS), and ICO (Windows) formats.
- **DMG creation fails on macOS:** Ensure `hdiutil` is available (it ships with macOS). If on a CI runner, make sure the macOS version supports DMG creation.
