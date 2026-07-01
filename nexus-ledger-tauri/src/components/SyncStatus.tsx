/**
 * SyncStatus — fixed-position edge-sync indicator for NexusLedger.
 *
 * Renders four visual states driven by `GET /api/v1/edge/status`:
 *
 *   1. Up to date  — green dot, "Last synced: X min ago"
 *   2. Syncing...  — yellow spinner + progress bar ("Pushing 47/100, Pulling 12")
 *   3. Offline     — orange dot, "Offline — N changes pending", offline duration
 *   4. Sync error  — red dot, expandable error details, "Retry" button
 *
 * Polling cadence adapts: 5 s while online, 30 s while offline.
 * Manual sync button calls `POST /api/v1/edge/sync` with a 3 s debounce.
 *
 * The component is safe to mount globally: it renders `null` when the
 * current user is not authenticated, so it never fires API calls before
 * login.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { apiGet, apiPost } from "../lib/api";
import { useAuth } from "../contexts/AuthContext";

/* ------------------------------------------------------------------ */
/*  Types                                                              */
/* ------------------------------------------------------------------ */

interface SyncProgress {
  pushed: number;
  push_total: number;
  pulled: number;
  pull_total: number;
}

interface EdgeStatus {
  enabled: boolean;
  offline_mode: boolean;
  is_online: boolean;
  last_sync: string | null; // ISO timestamp
  sync_in_progress: boolean;
  pending_changes: number;
  conflicts: number;
  sync_progress?: SyncProgress;
  error?: string | null;
}

/* ------------------------------------------------------------------ */
/*  Styling constants                                                  */
/* ------------------------------------------------------------------ */

const COLORS = {
  bg: "#16213e",
  border: "#2a2a4a",
  text: "#e0e0e0",
  textMuted: "#8888aa",
  green: "#4caf50",
  yellow: "#ffc107",
  orange: "#ff9800",
  red: "#f44336",
  amber: "#ffb300",
  btnBg: "#1e2d4a",
  btnHover: "#2a3d5e",
  progressBarBg: "#0d1730",
  expandBg: "#0f1a30",
};

const CONTAINER_STYLE: React.CSSProperties = {
  position: "fixed",
  bottom: "16px",
  right: "16px",
  width: "280px",
  zIndex: 1000,
  background: COLORS.bg,
  border: `1px solid ${COLORS.border}`,
  borderRadius: "8px",
  padding: "12px 14px",
  color: COLORS.text,
  fontFamily: "-apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif",
  fontSize: "13px",
  boxShadow: "0 4px 16px rgba(0,0,0,0.4)",
};

const HEADER_ROW_STYLE: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: "8px",
  marginBottom: "4px",
};

const DOT_STYLE: React.CSSProperties = {
  width: "8px",
  height: "8px",
  borderRadius: "50%",
  flexShrink: 0,
};

const MUTED_TEXT_STYLE: React.CSSProperties = {
  color: COLORS.textMuted,
  fontSize: "11px",
};

const SYNC_BTN_STYLE: React.CSSProperties = {
  marginTop: "8px",
  width: "100%",
  padding: "6px 10px",
  background: COLORS.btnBg,
  border: `1px solid ${COLORS.border}`,
  borderRadius: "4px",
  color: COLORS.text,
  cursor: "pointer",
  fontSize: "12px",
};

const RETRY_BTN_STYLE: React.CSSProperties = {
  ...SYNC_BTN_STYLE,
  borderColor: COLORS.red,
  color: COLORS.red,
};

const PROGRESS_BAR_CONTAINER_STYLE: React.CSSProperties = {
  marginTop: "8px",
  width: "100%",
  height: "6px",
  background: COLORS.progressBarBg,
  borderRadius: "3px",
  overflow: "hidden",
};

const SPINNER_KEYFRAMES = `
@keyframes syncstatus-spin {
  to { transform: rotate(360deg); }
}
@keyframes syncstatus-fadein {
  from { opacity: 0; }
  to { opacity: 1; }
}
`;

/* ------------------------------------------------------------------ */
/*  Time-formatting helpers                                            */
/* ------------------------------------------------------------------ */

/**
 * Returns a human-readable "X min ago" / "X hours ago" / "X days ago"
 * string for the given ISO timestamp.  When `offline` is true the suffix
 * " (offline)" is appended, as the spec requires for stale last-sync
 * display while disconnected.
 */
function formatLastSync(iso: string | null, offline: boolean): string {
  if (!iso) {
    return offline ? "Never (offline)" : "Never";
  }
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) {
    return offline ? "Unknown (offline)" : "Unknown";
  }
  const diffMs = Date.now() - then;
  const diffMin = Math.floor(diffMs / 60000);

  let core: string;
  if (diffMin < 1) {
    core = "just now";
  } else if (diffMin === 1) {
    core = "1 min ago";
  } else if (diffMin < 60) {
    core = `${diffMin} min ago`;
  } else {
    const diffHours = Math.floor(diffMin / 60);
    if (diffHours < 24) {
      const remainder = diffMin % 60;
      core =
        remainder > 0
          ? `${diffHours}h ${remainder}m ago`
          : `${diffHours}h ago`;
    } else {
      const diffDays = Math.floor(diffHours / 24);
      core = diffDays === 1 ? "1 day ago" : `${diffDays} days ago`;
    }
  }
  return offline ? `${core} (offline)` : core;
}

/**
 * Returns a compact offline-duration string like "Offline for 2h 15m"
 * or "Offline for 3m".  Falls back to "Offline" when the start time is
 * unknown.
 */
function formatOfflineDuration(offlineSince: number | null): string {
  if (offlineSince === null) return "Offline";
  const diffMs = Date.now() - offlineSince;
  const totalMin = Math.floor(diffMs / 60000);
  if (totalMin < 1) return "Offline for <1m";
  if (totalMin < 60) return `Offline for ${totalMin}m`;
  const hours = Math.floor(totalMin / 60);
  const mins = totalMin % 60;
  if (hours < 24) {
    return mins > 0 ? `Offline for ${hours}h ${mins}m` : `Offline for ${hours}h`;
  }
  const days = Math.floor(hours / 24);
  const remHours = hours % 24;
  return remHours > 0
    ? `Offline for ${days}d ${remHours}h`
    : `Offline for ${days}d`;
}

/**
 * Formats an error timestamp like "10:32 AM" for display in the error
 * details section.
 */
function formatErrorTime(d: Date): string {
  return d.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
}

/* ------------------------------------------------------------------ */
/*  Spinner                                                            */
/* ------------------------------------------------------------------ */

function Spinner({ color }: { color: string }) {
  return (
    <span
      style={{
        display: "inline-block",
        width: "12px",
        height: "12px",
        border: `2px solid ${color}40`,
        borderTopColor: color,
        borderRadius: "50%",
        flexShrink: 0,
        animation: "syncstatus-spin 0.8s linear infinite",
      }}
    />
  );
}

/* ------------------------------------------------------------------ */
/*  Component                                                          */
/* ------------------------------------------------------------------ */

const ONLINE_POLL_MS = 5_000;
const OFFLINE_POLL_MS = 30_000;
const SYNC_DEBOUNCE_MS = 3_000;
const PENDING_WARNING_THRESHOLD = 5_000;

export default function SyncStatus() {
  const { isAuthenticated } = useAuth();

  const [status, setStatus] = useState<EdgeStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [errorExpanded, setErrorExpanded] = useState(false);
  const [syncBtnDisabled, setSyncBtnDisabled] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);

  // Tracks when we first observed an offline state, for the duration counter.
  const offlineSinceRef = useRef<number | null>(null);

  // Guards against overlapping poll / sync-triggered fetches.
  const pollingRef = useRef(false);

  /* -------------------------------------------------------------- */
  /*  Fetch status                                                  */
  /* -------------------------------------------------------------- */

  const fetchStatus = useCallback(async () => {
    if (pollingRef.current) return;
    pollingRef.current = true;
    try {
      const data = await apiGet<EdgeStatus>("/api/v1/edge/status");
      setStatus(data);
      setActionError(null);

      // Manage offline-since timestamp for the duration counter.
      if (!data.is_online || data.offline_mode) {
        if (offlineSinceRef.current === null) {
          offlineSinceRef.current = Date.now();
        }
      } else {
        offlineSinceRef.current = null;
      }
    } catch {
      // Network failure means we are effectively offline.
      setStatus((prev) => {
        if (prev) {
          // Preserve last known values but mark offline.
          if (offlineSinceRef.current === null) {
            offlineSinceRef.current = Date.now();
          }
          return { ...prev, is_online: false, offline_mode: true };
        }
        // No prior status — synthesise a minimal offline state.
        if (offlineSinceRef.current === null) {
          offlineSinceRef.current = Date.now();
        }
        return {
          enabled: true,
          offline_mode: true,
          is_online: false,
          last_sync: null,
          sync_in_progress: false,
          pending_changes: 0,
          conflicts: 0,
        };
      });
    } finally {
      pollingRef.current = false;
      setLoading(false);
    }
  }, []);

  /* -------------------------------------------------------------- */
  /*  Polling with adaptive cadence                                 */
  /* -------------------------------------------------------------- */

  useEffect(() => {
    if (!isAuthenticated) return;

    // Initial fetch immediately on mount / when auth becomes available.
    fetchStatus();

    // Set up an adaptive interval.  We use a recursive setTimeout so each
    // cycle can pick the correct cadence based on the latest status.
    let timer: ReturnType<typeof setTimeout>;

    const scheduleNext = () => {
      const isOffline =
        status === null || !status.is_online || status.offline_mode;
      const delay = isOffline ? OFFLINE_POLL_MS : ONLINE_POLL_MS;
      timer = setTimeout(async () => {
        await fetchStatus();
        scheduleNext();
      }, delay);
    };
    scheduleNext();

    return () => clearTimeout(timer);
    // We intentionally re-evaluate when the online/offline flag changes so
    // the cadence switches.  `fetchStatus` is stable (useCallback with []).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isAuthenticated, status?.is_online, status?.offline_mode, fetchStatus]);

  /* -------------------------------------------------------------- */
  /*  Offline duration ticker (updates the displayed duration)      */
  /* -------------------------------------------------------------- */

  const [, setTick] = useState(0);
  useEffect(() => {
    if (offlineSinceRef.current === null) return;
    const id = setInterval(() => setTick((t) => t + 1), 30_000);
    return () => clearInterval(id);
  }, [offlineSinceRef.current]);

  /* -------------------------------------------------------------- */
  /*  Manual sync                                                   */
  /* -------------------------------------------------------------- */

  const handleSync = useCallback(async () => {
    if (syncBtnDisabled) return;
    setSyncBtnDisabled(true);
    setErrorExpanded(false);
    setActionError(null);

    // Optimistically mark sync-in-progress for immediate UI feedback.
    setStatus((prev) =>
      prev ? { ...prev, sync_in_progress: true, error: null } : prev,
    );

    try {
      await apiPost("/api/v1/edge/sync");
      // Immediately re-fetch to reflect the result.
      await fetchStatus();
    } catch (err) {
      const msg = err instanceof Error ? err.message : "Sync request failed";
      setActionError(msg);
      setStatus((prev) =>
        prev ? { ...prev, sync_in_progress: false, error: msg } : prev,
      );
      await fetchStatus();
    } finally {
      // 3 s debounce: keep the button disabled briefly after completion.
      setTimeout(() => setSyncBtnDisabled(false), SYNC_DEBOUNCE_MS);
    }
  }, [syncBtnDisabled, fetchStatus]);

  /* -------------------------------------------------------------- */
  /*  Inject keyframes once (guarded against duplication)           */
  /* -------------------------------------------------------------- */

  useEffect(() => {
    const id = "syncstatus-keyframes";
    if (document.getElementById(id)) return;
    const style = document.createElement("style");
    style.id = id;
    style.textContent = SPINNER_KEYFRAMES;
    document.head.appendChild(style);
    return () => {
      const el = document.getElementById(id);
      if (el) el.remove();
    };
  }, []);

  /* -------------------------------------------------------------- */
  /*  Render guards                                                 */
  /* -------------------------------------------------------------- */

  if (!isAuthenticated) return null;

  if (loading && !status) {
    // Minimal placeholder while the first fetch is in flight.
    return (
      <div style={CONTAINER_STYLE}>
        <div style={HEADER_ROW_STYLE}>
          <Spinner color={COLORS.textMuted} />
          <span style={MUTED_TEXT_STYLE}>Checking sync status…</span>
        </div>
      </div>
    );
  }

  if (!status) return null;

  // If edge sync is disabled entirely, show a muted note.
  if (!status.enabled) {
    return (
      <div style={CONTAINER_STYLE}>
        <div style={HEADER_ROW_STYLE}>
          <span style={{ ...DOT_STYLE, background: COLORS.textMuted }} />
          <span>Sync disabled</span>
        </div>
        <div style={MUTED_TEXT_STYLE}>
          Edge sync is not enabled for this device.
        </div>
      </div>
    );
  }

  /* -------------------------------------------------------------- */
  /*  Determine the active visual state                             */
  /* -------------------------------------------------------------- */

  const hasError = !!status.error || !!actionError;
  const isOffline = !status.is_online || status.offline_mode;
  const isSyncing = status.sync_in_progress && !hasError;

  const effectiveError = actionError ?? status.error ?? null;

  /* -------------------------------------------------------------- */
  /*  State: Sync error                                             */
  /* -------------------------------------------------------------- */

  if (hasError && !isSyncing) {
    return (
      <div style={CONTAINER_STYLE}>
        <div style={HEADER_ROW_STYLE}>
          <span style={{ ...DOT_STYLE, background: COLORS.red }} />
          <span style={{ color: COLORS.red, fontWeight: 600 }}>Sync error</span>
        </div>

        <div
          onClick={() => setErrorExpanded((v) => !v)}
          style={{
            cursor: "pointer",
            color: COLORS.textMuted,
            fontSize: "11px",
            userSelect: "none",
            marginTop: "4px",
          }}
        >
          {errorExpanded ? "▼ Hide details" : "▶ Show details"}
        </div>

        {errorExpanded && (
          <div
            style={{
              marginTop: "6px",
              padding: "8px",
              background: COLORS.expandBg,
              borderRadius: "4px",
              fontSize: "11px",
              color: COLORS.textMuted,
              animation: "syncstatus-fadein 0.15s ease",
            }}
          >
            {effectiveError ?? "Unknown error"}
            <div style={{ marginTop: "4px", fontSize: "10px" }}>
              {formatErrorTime(new Date())}
            </div>
          </div>
        )}

        {status.pending_changes > 0 && (
          <div style={{ ...MUTED_TEXT_STYLE, marginTop: "6px" }}>
            {status.pending_changes.toLocaleString()} change
            {status.pending_changes === 1 ? "" : "s"} pending
          </div>
        )}

        <button
          style={RETRY_BTN_STYLE}
          onClick={handleSync}
          disabled={syncBtnDisabled}
        >
          {syncBtnDisabled ? "Retrying…" : "Retry"}
        </button>
      </div>
    );
  }

  /* -------------------------------------------------------------- */
  /*  State: Syncing                                                */
  /* -------------------------------------------------------------- */

  if (isSyncing) {
    const progress = status.sync_progress;
    let progressPct = 0;
    let progressLabel = "Syncing…";

    if (progress) {
      const pushPct =
        progress.push_total > 0
          ? (progress.pushed / progress.push_total) * 100
          : 100;
      const pullPct =
        progress.pull_total > 0
          ? (progress.pulled / progress.pull_total) * 100
          : 100;
      // Overall progress weights push and pull equally.
      progressPct = Math.round((pushPct + pullPct) / 2);

      const parts: string[] = [];
      if (progress.push_total > 0) {
        parts.push(`Pushing ${progress.pushed}/${progress.push_total}`);
      }
      if (progress.pull_total > 0) {
        parts.push(`Pulling ${progress.pulled}/${progress.pull_total} new records`);
      }
      progressLabel = parts.join(", ") || "Syncing…";
    }

    return (
      <div style={CONTAINER_STYLE}>
        <div style={HEADER_ROW_STYLE}>
          <Spinner color={COLORS.yellow} />
          <span style={{ color: COLORS.yellow, fontWeight: 600 }}>
            Syncing…
          </span>
        </div>

        <div style={{ ...MUTED_TEXT_STYLE, marginTop: "2px" }}>
          {progressLabel}
        </div>

        <div style={PROGRESS_BAR_CONTAINER_STYLE}>
          <div
            style={{
              width: `${progressPct}%`,
              height: "100%",
              background: COLORS.yellow,
              borderRadius: "3px",
              transition: "width 0.3s ease",
            }}
          />
        </div>

        {status.pending_changes > 0 && (
          <div style={{ ...MUTED_TEXT_STYLE, marginTop: "6px" }}>
            {status.pending_changes.toLocaleString()} pending
          </div>
        )}
      </div>
    );
  }

  /* -------------------------------------------------------------- */
  /*  State: Offline                                                */
  /* -------------------------------------------------------------- */

  if (isOffline) {
    const pending = status.pending_changes;
    const showQueueWarning = pending > PENDING_WARNING_THRESHOLD;

    return (
      <div style={CONTAINER_STYLE}>
        <div style={HEADER_ROW_STYLE}>
          <span style={{ ...DOT_STYLE, background: COLORS.orange }} />
          <span style={{ color: COLORS.orange, fontWeight: 600 }}>
            Offline — {pending.toLocaleString()} change
            {pending === 1 ? "" : "s"} pending
          </span>
        </div>

        <div style={{ ...MUTED_TEXT_STYLE, marginTop: "2px" }}>
          {formatOfflineDuration(offlineSinceRef.current)}
        </div>

        <div style={{ ...MUTED_TEXT_STYLE, marginTop: "2px" }}>
          Last synced: {formatLastSync(status.last_sync, true)}
        </div>

        {showQueueWarning && (
          <div
            style={{
              marginTop: "6px",
              padding: "6px 8px",
              background: `${COLORS.amber}18`,
              border: `1px solid ${COLORS.amber}50`,
              borderRadius: "4px",
              color: COLORS.amber,
              fontSize: "11px",
            }}
          >
            ⚠ {pending.toLocaleString()} pending changes. Connect to sync soon.
          </div>
        )}

        {status.conflicts > 0 && (
          <div
            style={{
              ...MUTED_TEXT_STYLE,
              marginTop: "6px",
              color: COLORS.red,
            }}
          >
            {status.conflicts} conflict
            {status.conflicts === 1 ? "" : "s"} to resolve
          </div>
        )}

        <button
          style={SYNC_BTN_STYLE}
          onClick={handleSync}
          disabled={syncBtnDisabled}
        >
          {syncBtnDisabled ? "Queued…" : "Retry Sync"}
        </button>
      </div>
    );
  }

  /* -------------------------------------------------------------- */
  /*  State: Up to date                                             */
  /* -------------------------------------------------------------- */

  const pending = status.pending_changes;
  const showQueueWarning = pending > PENDING_WARNING_THRESHOLD;

  return (
    <div style={CONTAINER_STYLE}>
      <div style={HEADER_ROW_STYLE}>
        <span style={{ ...DOT_STYLE, background: COLORS.green }} />
        <span style={{ color: COLORS.green, fontWeight: 600 }}>
          Up to date
        </span>
      </div>

      <div style={{ ...MUTED_TEXT_STYLE, marginTop: "2px" }}>
        Last synced: {formatLastSync(status.last_sync, false)}
      </div>

      {pending > 0 && (
        <div style={{ ...MUTED_TEXT_STYLE, marginTop: "2px" }}>
          {pending.toLocaleString()} change
          {pending === 1 ? "" : "s"} pending
        </div>
      )}

      {showQueueWarning && (
        <div
          style={{
            marginTop: "6px",
            padding: "6px 8px",
            background: `${COLORS.amber}18`,
            border: `1px solid ${COLORS.amber}50`,
            borderRadius: "4px",
            color: COLORS.amber,
            fontSize: "11px",
          }}
        >
          ⚠ {pending.toLocaleString()} pending changes. Connect to sync soon.
        </div>
      )}

      {status.conflicts > 0 && (
        <div
          style={{
            ...MUTED_TEXT_STYLE,
            marginTop: "6px",
            color: COLORS.red,
          }}
        >
          {status.conflicts} conflict
          {status.conflicts === 1 ? "" : "s"} to resolve
        </div>
      )}

      <button
        style={SYNC_BTN_STYLE}
        onClick={handleSync}
        disabled={syncBtnDisabled}
        onMouseEnter={(e) => {
          if (!syncBtnDisabled)
            e.currentTarget.style.background = COLORS.btnHover;
        }}
        onMouseLeave={(e) => {
          e.currentTarget.style.background = COLORS.btnBg;
        }}
      >
        {syncBtnDisabled ? "Syncing…" : "Sync Now"}
      </button>
    </div>
  );
}
