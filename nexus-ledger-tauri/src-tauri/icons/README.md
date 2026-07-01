# NexusLedger Application Icons

This directory contains the icon files referenced by `tauri.conf.json`.

## Quick start — generate placeholder icons

If `icon.png` and `icon.ico` are not present yet, run the included
generator script (requires .NET / PowerShell on Windows):

```sh
powershell -ExecutionPolicy Bypass -File generate-icons.ps1
```

This creates a 256×256 brand-colour PNG with a "NL" overlay and a 32×32 ICO
derived from it — enough for development builds.

## Generating production-quality icons

Tauri can generate all required icon formats from a single source PNG of
1024×1024 or larger. Run:

```sh
cd nexus-ledger-tauri
npm run tauri icon path/to/source-icon.png
```

Or directly:

```sh
cargo tauri icon path/to/source-icon.png
```

This creates:
- `icon.png` — 512×512 PNG
- `icon.ico` — Windows ICO (multi-resolution)
- `32x32.png`, `128x128.png`, `128x128@2x.png` — additional sizes
- `icon.icns` — macOS app icon (if building on macOS)

## Requirements

- Source image should be at least 512×512 pixels (1024×1024 recommended).
- PNG format with transparency is preferred.
- The icon represents the NexusLedger brand identity.
