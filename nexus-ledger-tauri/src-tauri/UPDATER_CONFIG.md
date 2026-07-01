# Tauri Updater Configuration

This file documents the configuration that **must be added to `tauri.conf.json`**
for the auto-update functionality in `lib.rs` to work.

Another agent is handling `tauri.conf.json` modifications, so this file serves
as the specification for what needs to be added.

## Configuration to add

Add the following to `tauri.conf.json` under the `"plugins"` key:

```json
{
  "plugins": {
    "updater": {
      "active": true,
      "endpoints": [
        "https://github.com/RichdaleAccounting/NexusLedger/releases/latest/download/latest.json"
      ],
      "dialog": false,
      "pubkey": "REPLACE_WITH_SIGNING_KEY"
    }
  }
}
```

## Field reference

| Field         | Description                                                                                              |
|---------------|----------------------------------------------------------------------------------------------------------|
| `active`      | Set to `true` to enable the updater plugin.                                                              |
| `endpoints`   | Array of URLs serving the `latest.json` update manifest. The first reachable endpoint is used.           |
| `dialog`      | Set to `false` — we handle the update flow in the frontend (custom notification/prompt), not the native dialog. |
| `pubkey`      | The **public** key corresponding to the signing key used to sign update artifacts. **You must generate a keypair** and replace `REPLACE_WITH_SIGNING_KEY` with the real public key. |

## Generating the signing keypair

The Tauri updater uses **minisign** for signature verification. Generate a
keypair using the Tauri CLI:

```bash
# Generate a signing keypair (produces a .pub public key and a secret key)
npx @tauri-apps/cli signer generate -w ~/.tauri/nexus-ledger.key

# Output:
#   Public key: <PASTE INTO tauri.conf.json "pubkey">
#   Secret key: ~/.tauri/nexus-ledger.key  (set as TAURI_SIGNING_PRIVATE_KEY env var during builds)
```

Set the **private** key as an environment variable during CI/release builds:

```bash
export TAURI_SIGNING_PRIVATE_KEY=$(cat ~/.tauri/nexus-ledger.key)
```

## `latest.json` manifest format

The endpoint must serve a JSON file matching this schema:

```json
{
  "version": "1.0.1",
  "notes": "Release notes / changelog text",
  "pub_date": "2026-07-01T12:00:00Z",
  "platforms": {
    "darwin-x86_64": {
      "signature": "...",
      "url": "https://github.com/RichdaleAccounting/NexusLedger/releases/download/v1.0.1/NexusLedger.app.tar.gz"
    },
    "darwin-aarch64": {
      "signature": "...",
      "url": "https://github.com/.../NexusLedger.app.tar.gz"
    },
    "linux-x86_64": {
      "signature": "...",
      "url": "https://github.com/.../nexus-ledger_amd64.AppImage.tar.gz"
    },
    "windows-x86_64": {
      "signature": "...",
      "url": "https://github.com/.../NexusLedger-setup.exe"
    }
  }
}
```

## How the update flow works (lib.rs)

1. **Automatic check on startup** — 5 seconds after the app launches,
   `spawn_update_checker()` calls `check_for_updates_background()` which
   queries the endpoint. If an update is available, an `update-available`
   event is emitted to the frontend via `app.emit()`.

2. **Periodic checks** — Every 4 hours, the background task re-checks
   for updates automatically.

3. **Manual check** — The frontend can invoke the `check_for_updates`
   Tauri command, which returns `Option<UpdateInfo>` (Some if an update
   is available, None if up-to-date).

4. **Download & install** — The frontend calls `download_and_install_update`
   when the user accepts the update. This downloads the artifact, verifies
   the signature using `pubkey`, installs it, and calls `app.restart()`.

## Frontend integration (for reference — not part of this task)

The frontend should listen for the `update-available` event:

```typescript
import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';

interface UpdateInfo {
  version: string;
  current_version: string;
  date: string | null;
  body: string | null;
}

listen<UpdateInfo>('update-available', (event) => {
  const { version, body } = event.payload;
  // Show notification: "Update v{version} available"
  // On user acceptance:
  //   await invoke('download_and_install_update');
});
```
