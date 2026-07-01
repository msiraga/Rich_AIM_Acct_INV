/**
 * SyncStatus — fixed-position edge-sync indicator for NexusLedger.
 *
 * Polls `GET /api/v1/edge/status` every 5 seconds and renders a compact
 * pill in the bottom-right corner.  Four visual states:
 *
 *   1. online   — green dot, "Up to date", last-sync time
 *   2. syncing  — yellow spinner, "Syncing..."
 *   3. offline  — orange dot, "Offline — N changes pending"
 *   4. error    — red dot, "Sync error" + "Retry Sync" button
 *
 * On hover the pill expands to reveal last-sync time, storage usage,
 * pending-change count, and (when applicable) a "Sync Now" button.
 * If the status endpoint itself is unreachable, a red
 * "Connection error" indicator is shown instead.
 */

import { useState, useEffect, useCallback, useRef } from "react";
import { apiGet, apiPost } from "../lib/api";

/* ── Types ─────────────────────────────────────────────────────────── */

interface EdgeSyncStatus {
  enabled: boolean;
  offline_mode: boolean;
  is_online: boolean;
  last_sync: string | null;
  sync_in_progress: boolean;
  pending_changes: number;
  storage_used_mb: number;
  storage_max_mb: number;
}

interface ApiResponse<T> {
  success: boolean;
  data: T;
  error?: string;
}

interface SyncResult {
  synced: number;
  errors: number;
}

type SyncState = "online" | "syncing" | "offline" | "error";

/* ── Constants ─────────────────────────────────────────────────────── */

const POLL_INTERVAL_MS = 5000;

/**
 * Inline style fallbacks for dot colours.  The corresponding CSS classes
 * (`.sync-dot-green` etc.) will be defined centrally in index.css; these
 * inline values guarantee the indicator is visible even before that CSS
 * is wired in.  CSS classes can override by using `!important` if a
 * different palette is desired.
 */
const DOT_COLORS: Record<string, string> = {
  "sync-dot-green": "#4ade80",
  "sync-dot-yellow": "#fbbf24",
  "sync-dot-orange": "#f97316",
  "sync-dot-red": "#f87171",
};

/* ── Helpers ───────────────────────────────────────────────────────── */

/** Compact relative-time string for the pill (e.g. "5m ago", "Jul 1"). */
function formatLastSyncShort(iso: string | null): string {
  if (!iso) return "";
  const date = new Date(iso);
  if (isNaN(date.getTime())) return "";

  const diffMs = Date.now() - date.getTime();
  const diffMin = Math.floor(diffMs / 60000);

  if (diffMin < 1) return "just now";
  if (diffMin < 60) return `${diffMin}m ago`;

  const diffHr = Math.floor(diffMin / 60);
  if (diffHr < 24) return `${diffHr}h ago`;

  return date.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

/** Full timestamp string for the detail panel and aria-label. */
function formatLastSyncFull(iso: string | null): string {
  if (!iso) return "Never";
  const date = new Date(iso);
  if (isNaN(date.getTime())) return "Never";
  return date.toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

/** Storage usage as "used / max (pct%)". */
function formatStorage(usedMb: number, maxMb: number): string {
  if (maxMb <= 0) return `${usedMb} MB used`;
  const pct = Math.round((usedMb / maxMb) * 100);
  return `${usedMb} / ${maxMb} MB (${pct}%)`;
}

/* ── Component ─────────────────────────────────────────────────────── */

function SyncStatus() {
  const [status, setStatus] = useState<EdgeSyncStatus | null>(null);
  const [connectionError, setConnectionError] = useState(false);
  const [manualSyncing, setManualSyncing] = useState(false);
  const [lastSyncFailed, setLastSyncFailed] = useState(false);
  const [hovered, setHovered] = useState(false);

  // Guards against state updates after unmount.
  const mountedRef = useRef(true);

  /* ── Status polling ────────────────────────────────────────────── */

  const fetchStatus = useCallback(async () => {
    try {
      const res = await apiGet<ApiResponse<EdgeSyncStatus>>(
        "/api/v1/edge/status",
      );
      if (!mountedRef.current) return;

      if (res.success && res.data) {
        setStatus(res.data);
        setConnectionError(false);
        // If the backend reports zero pending after a prior failure, clear it.
        if (res.data.pending_changes === 0) {
          setLastSyncFailed(false);
        }
      } else {
        setConnectionError(true);
      }
    } catch {
      if (mountedRef.current) {
        setConnectionError(true);
      }
    }
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    fetchStatus();

    const intervalId = setInterval(fetchStatus, POLL_INTERVAL_MS);

    return () => {
      mountedRef.current = false;
      clearInterval(intervalId);
    };
  }, [fetchStatus]);

  /* ── Manual sync ───────────────────────────────────────────────── */

  const handleManualSync = useCallback(async () => {
    setManualSyncing(true);
    try {
      const res = await apiPost<ApiResponse<SyncResult>>(
        "/api/v1/edge/sync",
      );
      if (!mountedRef.current) return;

      if (res.success && res.data) {
        setLastSyncFailed(res.data.errors > 0);
      } else {
        setLastSyncFailed(true);
      }
    } catch {
      if (mountedRef.current) {
        setLastSyncFailed(true);
      }
    } finally {
      if (mountedRef.current) {
        setManualSyncing(false);
        // Immediately re-fetch status after the sync attempt.
        fetchStatus();
      }
    }
  }, [fetchStatus]);

  /* ── Derive display state ──────────────────────────────────────── */

  const isSyncing =
    (status?.sync_in_progress ?? false) || manualSyncing;

  // Determine the high-level state.  Early-return for the "still loading"
  // case so we don't flash a stale indicator.
  let syncState: SyncState;
  if (connectionError) {
    syncState = "error";
  } else if (isSyncing) {
    syncState = "syncing";
  } else if (!status) {
    return null; // Initial load — nothing to show yet.
  } else if (!status.is_online || status.offline_mode) {
    syncState = "offline";
  } else if (status.pending_changes > 0 && lastSyncFailed) {
    syncState = "error";
  } else {
    syncState = "online";
  }

  // Hide entirely when edge sync is disabled.
  if (status && !status.enabled) return null;

  const pending = status?.pending_changes ?? 0;
  const lastSyncFull = formatLastSyncFull(status?.last_sync ?? null);
  const lastSyncShort = formatLastSyncShort(status?.last_sync ?? null);
  const storage = status
    ? formatStorage(status.storage_used_mb, status.storage_max_mb)
    : "";
  const pendingLabel = `${pending} ${pending === 1 ? "change" : "changes"}`;

  /* ── State-specific labels ─────────────────────────────────────── */

  let dotClass: string;
  let pillText: string;
  let ariaLabel: string;

  switch (syncState) {
    case "online":
      dotClass = "sync-dot-green";
      pillText = lastSyncShort
        ? `Up to date · ${lastSyncShort}`
        : "Up to date";
      ariaLabel = `Sync up to date. Last synced ${lastSyncFull}.`;
      break;
    case "syncing":
      dotClass = "sync-dot-yellow";
      pillText = "Syncing...";
      ariaLabel = "Sync in progress.";
      break;
    case "offline":
      dotClass = "sync-dot-orange";
      pillText = `Offline — ${pending} ${pending === 1 ? "change" : "changes"} pending`;
      ariaLabel = `Offline. ${pending} changes pending sync.`;
      break;
    case "error":
      dotClass = "sync-dot-red";
      if (connectionError) {
        pillText = "Connection error";
        ariaLabel = "Cannot reach sync service.";
      } else {
        pillText = "Sync error";
        ariaLabel = `Sync error. ${pending} changes pending.`;
      }
      break;
  }

  const showRetryButton = syncState === "error" && !connectionError;
  const showSyncNowDetail = syncState === "online" && pending > 0;
  const canSync = !isSyncing && !connectionError;

  /* ── Render ────────────────────────────────────────────────────── */

  return (
    <div
      className={`sync-status${hovered ? " sync-status-expanded" : ""}`}
      role="status"
      aria-live="polite"
      aria-label={ariaLabel}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      style={{
        position: "fixed",
        bottom: "16px",
        right: "76px",
        zIndex: 998,
      }}
    >
      {/* ── Pill (always visible) ──────────────────────────────── */}
      <div
        className="sync-status-pill"
        style={{
          display: "inline-flex",
          alignItems: "center",
          gap: "8px",
          padding: "6px 14px",
          borderRadius: "20px",
          backgroundColor: "#16213e",
          border: "1px solid #2a2a4a",
          fontSize: "13px",
          lineHeight: 1,
          whiteSpace: "nowrap",
          transition: "all 0.2s ease",
          cursor: "default",
          userSelect: "none",
        }}
      >
        {syncState === "syncing" ? (
          <span
            className="sync-status-dot sync-spinner sync-dot-yellow"
            style={{
              width: "10px",
              height: "10px",
              borderRadius: "50%",
              display: "inline-block",
              flexShrink: 0,
              backgroundColor: DOT_COLORS["sync-dot-yellow"],
            }}
            aria-hidden="true"
          />
        ) : (
          <span
            className={`sync-status-dot ${dotClass}`}
            style={{
              width: "8px",
              height: "8px",
              borderRadius: "50%",
              display: "inline-block",
              flexShrink: 0,
              backgroundColor: DOT_COLORS[dotClass],
            }}
            aria-hidden="true"
          />
        )}

        <span className="sync-status-text">{pillText}</span>

        {showRetryButton && (
          <button
            className="sync-status-button"
            onClick={handleManualSync}
            disabled={!canSync}
            aria-label="Retry sync"
            style={{
              marginLeft: "4px",
              padding: "2px 10px",
              fontSize: "12px",
              fontWeight: 600,
              borderRadius: "12px",
              border: "1px solid #5a2a2a",
              backgroundColor: "#3a1a1a",
              color: "#f87171",
              cursor: canSync ? "pointer" : "default",
              transition: "all 0.15s ease",
            }}
          >
            {manualSyncing ? "Retrying..." : "Retry Sync"}
          </button>
        )}
      </div>

      {/* ── Detail panel (on hover) ────────────────────────────── */}
      {hovered && status && !connectionError && (
        <div
          className="sync-status-detail"
          role="tooltip"
          style={{
            position: "absolute",
            bottom: "100%",
            right: "0",
            marginBottom: "8px",
            padding: "12px 16px",
            borderRadius: "8px",
            backgroundColor: "#16213e",
            border: "1px solid #2a2a4a",
            fontSize: "13px",
            lineHeight: 1.6,
            whiteSpace: "nowrap",
            boxShadow: "0 4px 16px rgba(0, 0, 0, 0.4)",
            transition: "opacity 0.2s ease",
          }}
        >
          <div
            className="sync-status-detail-row"
            style={{
              display: "flex",
              justifyContent: "space-between",
              gap: "16px",
            }}
          >
            <span style={{ color: "#888" }}>Last sync</span>
            <span style={{ color: "#e0e0ff" }}>{lastSyncFull}</span>
          </div>

          <div
            className="sync-status-detail-row"
            style={{
              display: "flex",
              justifyContent: "space-between",
              gap: "16px",
            }}
          >
            <span style={{ color: "#888" }}>Storage</span>
            <span style={{ color: "#e0e0ff" }}>{storage}</span>
          </div>

          {pending > 0 && (
            <div
              className="sync-status-detail-row"
              style={{
                display: "flex",
                justifyContent: "space-between",
                gap: "16px",
              }}
            >
              <span style={{ color: "#888" }}>Pending</span>
              <span style={{ color: "#fbbf24" }}>{pendingLabel}</span>
            </div>
          )}

          {showSyncNowDetail && (
            <button
              className="sync-status-button"
              onClick={handleManualSync}
              disabled={!canSync}
              aria-label="Sync now"
              style={{
                marginTop: "8px",
                width: "100%",
                padding: "6px 12px",
                fontSize: "12px",
                fontWeight: 600,
                borderRadius: "6px",
                border: "1px solid #3a3a5a",
                backgroundColor: "#2a2a4a",
                color: "#a0a0c0",
                cursor: canSync ? "pointer" : "default",
                transition: "all 0.15s ease",
              }}
            >
              Sync Now
            </button>
          )}
        </div>
      )}
    </div>
  );
}

export default SyncStatus;
