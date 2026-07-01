/**
 * ConflictResolver — side-by-side sync conflict resolution UI for NexusLedger.
 *
 * Fetches pending edge-sync conflicts from `GET /api/v1/edge/conflicts` and
 * renders each as a collapsible card with a three-column diff table
 * (Field | Local | Remote).  Fields listed in `diff_fields` are highlighted
 * with a yellow tint so the user can instantly see what diverged.
 *
 * Each conflict can be resolved individually with one of:
 *   • "Keep Local"   — discard remote, accept local version
 *   • "Keep Remote"  — discard local, accept remote version
 *   • "Keep Both"    — merge non-conflicting field changes (resolution: "merged")
 *
 * When two or more conflicts are pending, a batch-resolve bar appears at the
 * top offering "Resolve all with: [Keep Local | Keep Remote]".
 *
 * Resolved conflicts are removed optimistically; if the server rejects the
 * resolution the list is re-fetched so the conflict reappears.  A toast
 * notification confirms each action.
 *
 * Dark-theme colours mirror the palette in index.css:
 *   #1a1a2e base bg · #16213e panel · #2a2a4a border · #1f2f4f hover
 *   #7c8aff accent · #e0e0ff text · #a0a0c0 secondary · #888 muted
 *   #4ade80 success · #f87171 error · #fbbf24 warning
 */

import { useState, useEffect, useCallback, useRef } from "react";
import type { CSSProperties } from "react";
import { apiGet, apiPost } from "../lib/api";

/* ── Types ─────────────────────────────────────────────────────────── */

interface Conflict {
  id: string;
  entity_type: string;
  entity_id: string;
  local_version: Record<string, any>;
  remote_version: Record<string, any>;
  diff_fields: string[];
  local_modified_at: string;
  remote_modified_at: string;
  /** Optional — populated when the backend tracks the modifying user. */
  local_modified_by?: string;
  remote_modified_by?: string;
  status: "pending" | "resolved";
}

interface ApiResponse<T> {
  success: boolean;
  data: T;
  error?: string;
}

type Resolution = "local" | "remote" | "merged";

interface Toast {
  id: number;
  type: "success" | "error";
  message: string;
}

/* ── Colour palette (mirrors index.css) ────────────────────────────── */

const C = {
  bg: "#1a1a2e",
  panel: "#16213e",
  border: "#2a2a4a",
  borderLight: "#3a3a5a",
  hover: "#1f2f4f",
  accent: "#7c8aff",
  text: "#e0e0ff",
  textSecondary: "#a0a0c0",
  textMuted: "#888",
  success: "#4ade80",
  successBg: "#1a3a2a",
  error: "#f87171",
  errorBg: "#3a1a1a",
  warning: "#fbbf24",
  warningBg: "#3a3a1a",
  /** Semi-transparent yellow for diff-row highlighting. */
  warningTint: "rgba(251, 191, 36, 0.12)",
} as const;

/* ── Constants ─────────────────────────────────────────────────────── */

/** Field names that may carry the modifier's identity inside a version record. */
const MODIFIER_FIELDS = [
  "modified_by",
  "updated_by",
  "user_id",
  "created_by",
  "owner_id",
  "username",
  "user",
] as const;

/** Field-name substrings that suggest a monetary value. */
const CURRENCY_HINTS = [
  "amount",
  "balance",
  "total",
  "subtotal",
  "tax",
  "discount",
  "price",
  "rate",
  "fee",
  "payment",
  "debit",
  "credit",
] as const;

/* ── Shared style fragments ────────────────────────────────────────── */

const sTh: CSSProperties = {
  textAlign: "left",
  padding: "8px 12px",
  fontSize: "11px",
  fontWeight: 600,
  textTransform: "uppercase",
  letterSpacing: "0.5px",
  color: C.textMuted,
  backgroundColor: C.hover,
  borderBottom: `1px solid ${C.border}`,
};

const sTd: CSSProperties = {
  padding: "8px 12px",
  verticalAlign: "top",
};

const sResolveBtn: CSSProperties = {
  padding: "8px 16px",
  fontSize: "13px",
  fontWeight: 600,
  borderRadius: "6px",
  border: "1px solid",
  cursor: "pointer",
  transition: "all 0.15s ease",
};

const sBatchBtn: CSSProperties = {
  padding: "5px 12px",
  fontSize: "12px",
  fontWeight: 600,
  borderRadius: "6px",
  border: `1px solid ${C.border}`,
  backgroundColor: "transparent",
  cursor: "pointer",
  transition: "all 0.15s ease",
};

/* ── Helpers ───────────────────────────────────────────────────────── */

/** Format an ISO timestamp into a human-readable string. */
function formatTimestamp(iso: string | null | undefined): string {
  if (!iso) return "Unknown";
  const date = new Date(iso);
  if (isNaN(date.getTime())) return "Invalid date";
  return date.toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

/**
 * Extract the modifier identity from a version record or the optional
 * top-level `*_modified_by` field on the Conflict object.
 */
function extractModifier(
  version: Record<string, any>,
  explicit?: string,
): string | null {
  if (explicit) return explicit;
  for (const field of MODIFIER_FIELDS) {
    const val = version[field];
    if (val !== null && val !== undefined && val !== "") {
      return String(val);
    }
  }
  return null;
}

/**
 * Format a field value for display in the diff table.
 * Handles booleans, numbers (currency-aware), ISO dates, and objects.
 */
function formatValue(fieldName: string, value: any): string {
  if (value === null || value === undefined) return "—";

  if (typeof value === "boolean") return value ? "Yes" : "No";

  if (typeof value === "number") {
    const lower = fieldName.toLowerCase();
    if (CURRENCY_HINTS.some((h) => lower.includes(h))) {
      return new Intl.NumberFormat(undefined, {
        style: "currency",
        currency: "USD",
      }).format(value);
    }
    return value.toLocaleString();
  }

  if (typeof value === "string") {
    // ISO date strings get pretty-formatted.
    if (/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}/.test(value)) {
      return formatTimestamp(value);
    }
    return value;
  }

  if (Array.isArray(value) || typeof value === "object") {
    try {
      return JSON.stringify(value);
    } catch {
      return String(value);
    }
  }

  return String(value);
}

/**
 * Collect all unique field names across local version, remote version,
 * and the diff_fields list.  diff_fields are placed first so the most
 * important differences appear at the top of the table.
 */
function collectFields(conflict: Conflict): string[] {
  const fields: string[] = [];
  const seen = new Set<string>();

  // Prioritise diff fields.
  for (const f of conflict.diff_fields) {
    if (!seen.has(f)) {
      seen.add(f);
      fields.push(f);
    }
  }
  // Then remaining fields from both versions.
  for (const key of Object.keys(conflict.local_version)) {
    if (!seen.has(key)) {
      seen.add(key);
      fields.push(key);
    }
  }
  for (const key of Object.keys(conflict.remote_version)) {
    if (!seen.has(key)) {
      seen.add(key);
      fields.push(key);
    }
  }

  return fields;
}

/** Human-readable label for an entity type (e.g. "journal_entry" → "Journal Entry"). */
function entityLabel(entityType: string): string {
  const known: Record<string, string> = {
    transaction: "Transaction",
    invoice: "Invoice",
    account: "Account",
    journal_entry: "Journal Entry",
    contact: "Contact",
    document: "Document",
    budget: "Budget",
    recurring_invoice: "Recurring Invoice",
  };
  if (known[entityType]) return known[entityType];
  return entityType
    .replace(/_/g, " ")
    .replace(/\b\w/g, (ch) => ch.toUpperCase());
}

/** Pretty-print a field name (e.g. "opening_balance" → "Opening Balance"). */
function prettyField(field: string): string {
  return field
    .replace(/_/g, " ")
    .replace(/\b\w/g, (ch) => ch.toUpperCase());
}

/* ── Toast container (sub-component) ───────────────────────────────── */

function ToastStack({ toasts }: { toasts: Toast[] }) {
  if (toasts.length === 0) return null;

  return (
    <div
      style={{
        position: "fixed",
        bottom: "20px",
        left: "50%",
        transform: "translateX(-50%)",
        display: "flex",
        flexDirection: "column",
        gap: "8px",
        zIndex: 10000,
        pointerEvents: "none",
      }}
    >
      {toasts.map((toast) => {
        const ok = toast.type === "success";
        return (
          <div
            key={toast.id}
            role="alert"
            style={{
              padding: "10px 20px",
              borderRadius: "8px",
              fontSize: "14px",
              fontWeight: 500,
              boxShadow: "0 4px 16px rgba(0, 0, 0, 0.4)",
              backgroundColor: ok ? C.successBg : C.errorBg,
              color: ok ? C.success : C.error,
              border: `1px solid ${ok ? C.success : C.error}40`,
              pointerEvents: "auto",
              transition: "opacity 0.2s ease",
            }}
          >
            {toast.message}
          </div>
        );
      })}
    </div>
  );
}

/* ── Main component ────────────────────────────────────────────────── */

function ConflictResolver() {
  const [conflicts, setConflicts] = useState<Conflict[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [resolvingIds, setResolvingIds] = useState<Set<string>>(new Set());
  const [batchResolving, setBatchResolving] = useState(false);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [expandedIds, setExpandedIds] = useState<Set<string>>(new Set());

  // Guards against state updates after unmount.
  const mountedRef = useRef(true);
  const toastIdRef = useRef(0);

  /* ── Toast helpers ────────────────────────────────────────────── */

  const addToast = useCallback((type: Toast["type"], message: string) => {
    const id = ++toastIdRef.current;
    setToasts((prev) => [...prev, { id, type, message }]);
    // Auto-dismiss after 4 seconds.
    setTimeout(() => {
      if (!mountedRef.current) return;
      setToasts((prev) => prev.filter((t) => t.id !== id));
    }, 4000);
  }, []);

  /* ── Fetch conflicts ──────────────────────────────────────────── */

  const fetchConflicts = useCallback(async () => {
    setLoading(true);
    setLoadError(null);
    try {
      const res = await apiGet<ApiResponse<Conflict[]>>(
        "/api/v1/edge/conflicts",
      );
      if (!mountedRef.current) return;

      if (res.success && Array.isArray(res.data)) {
        // Only show pending conflicts; resolved ones are filtered out.
        const pending = res.data.filter((c) => c.status === "pending");
        setConflicts(pending);
        // Auto-expand all on (re)load so the user sees everything.
        setExpandedIds(new Set(pending.map((c) => c.id)));
      } else {
        setConflicts([]);
      }
    } catch (err) {
      if (!mountedRef.current) return;
      setLoadError(
        err instanceof Error ? err.message : "Failed to load conflicts",
      );
    } finally {
      if (mountedRef.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    fetchConflicts();
    return () => {
      mountedRef.current = false;
    };
  }, [fetchConflicts]);

  /* ── Resolve a single conflict ────────────────────────────────── */

  const handleResolve = useCallback(
    async (conflictId: string, resolution: Resolution) => {
      setResolvingIds((prev) => new Set(prev).add(conflictId));

      // Optimistic removal — the conflict disappears immediately.
      setConflicts((prev) => prev.filter((c) => c.id !== conflictId));
      setExpandedIds((prev) => {
        const next = new Set(prev);
        next.delete(conflictId);
        return next;
      });

      try {
        const res = await apiPost<ApiResponse<{ id: string; status: string }>>(
          `/api/v1/edge/conflicts/${conflictId}/resolve`,
          { resolution },
        );
        if (!mountedRef.current) return;

        if (res.success) {
          const labels: Record<Resolution, string> = {
            local: "kept local version",
            remote: "kept remote version",
            merged: "merged both versions",
          };
          addToast("success", `Conflict resolved — ${labels[resolution]}.`);
        } else {
          // Server rejected — re-fetch to restore the conflict.
          addToast("error", res.error || "Failed to resolve conflict.");
          fetchConflicts();
        }
      } catch (err) {
        if (!mountedRef.current) return;
        addToast(
          "error",
          err instanceof Error ? err.message : "Failed to resolve conflict.",
        );
        fetchConflicts();
      } finally {
        if (mountedRef.current) {
          setResolvingIds((prev) => {
            const next = new Set(prev);
            next.delete(conflictId);
            return next;
          });
        }
      }
    },
    [addToast, fetchConflicts],
  );

  /* ── Batch resolve all pending ────────────────────────────────── */

  const handleBatchResolve = useCallback(
    async (resolution: Resolution) => {
      setBatchResolving(true);
      const toResolve = conflicts.map((c) => c.id);

      // Optimistic removal of all pending conflicts.
      setConflicts([]);
      setExpandedIds(new Set());

      try {
        const results = await Promise.allSettled(
          toResolve.map((id) =>
            apiPost<ApiResponse<{ id: string; status: string }>>(
              `/api/v1/edge/conflicts/${id}/resolve`,
              { resolution },
            ),
          ),
        );

        if (!mountedRef.current) return;

        const succeeded = results.filter(
          (r) => r.status === "fulfilled" && r.value.success,
        ).length;
        const failed = toResolve.length - succeeded;

        if (failed === 0) {
          addToast(
            "success",
            `${succeeded} conflict${succeeded === 1 ? "" : "s"} resolved.`,
          );
        } else {
          addToast(
            "error",
            `${succeeded} resolved, ${failed} failed. Remaining conflicts reloaded.`,
          );
          // Re-fetch to restore conflicts that failed to resolve.
          fetchConflicts();
        }
      } catch {
        if (!mountedRef.current) return;
        addToast("error", "Batch resolution failed.");
        fetchConflicts();
      } finally {
        if (mountedRef.current) setBatchResolving(false);
      }
    },
    [conflicts, addToast, fetchConflicts],
  );

  /* ── Toggle expand/collapse ───────────────────────────────────── */

  const toggleExpand = useCallback((id: string) => {
    setExpandedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  /* ── Derived state ────────────────────────────────────────────── */

  const pendingCount = conflicts.length;
  const hasMultiple = pendingCount > 1;

  /* ── Render: Loading ──────────────────────────────────────────── */

  if (loading) {
    return (
      <div className="conflict-resolver" style={{ padding: "40px", textAlign: "center" }}>
        <p style={{ color: C.textMuted, fontSize: "14px" }}>
          Loading conflicts…
        </p>
      </div>
    );
  }

  /* ── Render: Error ────────────────────────────────────────────── */

  if (loadError) {
    return (
      <div className="conflict-resolver" style={{ padding: "40px", textAlign: "center" }}>
        <p style={{ color: C.error, fontSize: "14px", marginBottom: "16px" }}>
          {loadError}
        </p>
        <button
          className="btn-secondary btn-small"
          onClick={fetchConflicts}
          style={{
            padding: "6px 16px",
            borderRadius: "6px",
            border: `1px solid ${C.border}`,
            backgroundColor: C.panel,
            color: C.textSecondary,
            cursor: "pointer",
            fontSize: "13px",
          }}
        >
          Retry
        </button>
      </div>
    );
  }

  /* ── Render: Empty state ──────────────────────────────────────── */

  if (pendingCount === 0) {
    return (
      <>
        <div
          className="conflict-resolver conflict-resolver-empty"
          style={{ padding: "60px 40px", textAlign: "center" }}
        >
          <div
            style={{ fontSize: "48px", marginBottom: "16px", opacity: 0.3 }}
            aria-hidden="true"
          >
            ✓
          </div>
          <h3
            style={{
              margin: "0 0 8px",
              fontSize: "18px",
              color: C.text,
            }}
          >
            No conflicts to resolve
          </h3>
          <p style={{ color: C.textMuted, fontSize: "14px", margin: 0 }}>
            All sync conflicts have been resolved. Your data is up to date.
          </p>
        </div>
        <ToastStack toasts={toasts} />
      </>
    );
  }

  /* ── Render: Conflict list ────────────────────────────────────── */

  return (
    <>
      <div
        className="conflict-resolver"
        style={{ display: "flex", flexDirection: "column", gap: "16px" }}
      >
        {/* ── Header + batch resolve bar ───────────────────────── */}
        <div
          className="conflict-resolver-header"
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            flexWrap: "wrap",
            gap: "12px",
          }}
        >
          <div style={{ display: "flex", alignItems: "center", gap: "10px" }}>
            <h2
              style={{ margin: 0, fontSize: "20px", color: C.text }}
            >
              Sync Conflicts
            </h2>
            <span
              style={{
                padding: "2px 10px",
                borderRadius: "12px",
                backgroundColor: C.warningBg,
                color: C.warning,
                fontSize: "13px",
                fontWeight: 600,
              }}
            >
              {pendingCount} pending
            </span>
          </div>

          {hasMultiple && !batchResolving && (
            <div
              className="conflict-batch-resolve"
              style={{
                display: "flex",
                alignItems: "center",
                gap: "8px",
              }}
            >
              <span style={{ color: C.textMuted, fontSize: "13px" }}>
                Resolve all with:
              </span>
              <button
                onClick={() => handleBatchResolve("local")}
                style={{
                  ...sBatchBtn,
                  borderColor: C.accent,
                  color: C.accent,
                }}
              >
                Keep Local
              </button>
              <button
                onClick={() => handleBatchResolve("remote")}
                style={{
                  ...sBatchBtn,
                  borderColor: C.accent,
                  color: C.accent,
                }}
              >
                Keep Remote
              </button>
            </div>
          )}

          {batchResolving && (
            <span style={{ color: C.textMuted, fontSize: "13px" }}>
              Resolving all conflicts…
            </span>
          )}
        </div>

        {/* ── Conflict cards ────────────────────────────────────── */}
        {conflicts.map((conflict) => {
          const isExpanded = expandedIds.has(conflict.id);
          const isResolving = resolvingIds.has(conflict.id);
          const fields = collectFields(conflict);
          const localModifier = extractModifier(
            conflict.local_version,
            conflict.local_modified_by,
          );
          const remoteModifier = extractModifier(
            conflict.remote_version,
            conflict.remote_modified_by,
          );

          return (
            <div
              key={conflict.id}
              className="conflict-card"
              style={{
                backgroundColor: C.panel,
                border: `1px solid ${C.border}`,
                borderRadius: "10px",
                overflow: "hidden",
              }}
            >
              {/* ── Card header (click to expand/collapse) ──────── */}
              <div
                className="conflict-card-header"
                onClick={() => toggleExpand(conflict.id)}
                role="button"
                tabIndex={0}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    toggleExpand(conflict.id);
                  }
                }}
                style={{
                  display: "flex",
                  justifyContent: "space-between",
                  alignItems: "center",
                  padding: "14px 18px",
                  cursor: "pointer",
                  userSelect: "none",
                  transition: "background-color 0.15s ease",
                }}
                onMouseEnter={(e) => {
                  (e.currentTarget as HTMLElement).style.backgroundColor =
                    C.hover;
                }}
                onMouseLeave={(e) => {
                  (e.currentTarget as HTMLElement).style.backgroundColor =
                    "transparent";
                }}
              >
                <div
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: "12px",
                  }}
                >
                  {/* Expand/collapse chevron */}
                  <span
                    aria-hidden="true"
                    style={{
                      fontSize: "14px",
                      color: C.textMuted,
                      display: "inline-block",
                      transition: "transform 0.2s ease",
                      transform: isExpanded
                        ? "rotate(90deg)"
                        : "rotate(0deg)",
                    }}
                  >
                    ▶
                  </span>

                  {/* Entity type badge */}
                  <span
                    style={{
                      padding: "3px 10px",
                      borderRadius: "6px",
                      backgroundColor: C.hover,
                      color: C.accent,
                      fontSize: "12px",
                      fontWeight: 600,
                      textTransform: "uppercase",
                      letterSpacing: "0.5px",
                    }}
                  >
                    {entityLabel(conflict.entity_type)}
                  </span>

                  {/* Entity ID */}
                  <span
                    style={{
                      color: C.textSecondary,
                      fontSize: "14px",
                    }}
                  >
                    ID:{" "}
                    <code
                      style={{
                        color: C.accent,
                        fontFamily: "monospace",
                        fontSize: "13px",
                      }}
                    >
                      {conflict.entity_id}
                    </code>
                  </span>
                </div>

                {/* Diff count badge */}
                <span
                  style={{
                    padding: "2px 8px",
                    borderRadius: "4px",
                    backgroundColor: C.warningBg,
                    color: C.warning,
                    fontSize: "11px",
                    fontWeight: 600,
                  }}
                >
                  {conflict.diff_fields.length} field
                  {conflict.diff_fields.length === 1 ? "" : "s"} differ
                </span>
              </div>

              {/* ── Card body (collapsible) ──────────────────────── */}
              {isExpanded && (
                <div
                  className="conflict-card-body"
                  style={{ padding: "0 18px 18px" }}
                >
                  {/* ── Audit info bar ──────────────────────────── */}
                  <div
                    className="conflict-audit-bar"
                    style={{
                      display: "flex",
                      gap: "24px",
                      flexWrap: "wrap",
                      padding: "10px 0",
                      marginBottom: "12px",
                      borderBottom: `1px solid ${C.border}`,
                      fontSize: "12px",
                    }}
                  >
                    <div
                      style={{
                        display: "flex",
                        flexDirection: "column",
                        gap: "2px",
                      }}
                    >
                      <span
                        style={{
                          color: C.textMuted,
                          textTransform: "uppercase",
                          letterSpacing: "0.5px",
                          fontSize: "10px",
                        }}
                      >
                        Local Version
                      </span>
                      <span style={{ color: C.textSecondary }}>
                        Modified {formatTimestamp(conflict.local_modified_at)}
                        {localModifier && (
                          <span style={{ color: C.textMuted }}>
                            {" "}
                            · by {localModifier}
                          </span>
                        )}
                      </span>
                    </div>
                    <div
                      style={{
                        display: "flex",
                        flexDirection: "column",
                        gap: "2px",
                      }}
                    >
                      <span
                        style={{
                          color: C.textMuted,
                          textTransform: "uppercase",
                          letterSpacing: "0.5px",
                          fontSize: "10px",
                        }}
                      >
                        Remote Version
                      </span>
                      <span style={{ color: C.textSecondary }}>
                        Modified{" "}
                        {formatTimestamp(conflict.remote_modified_at)}
                        {remoteModifier && (
                          <span style={{ color: C.textMuted }}>
                            {" "}
                            · by {remoteModifier}
                          </span>
                        )}
                      </span>
                    </div>
                  </div>

                  {/* ── Side-by-side diff table ─────────────────── */}
                  <div style={{ overflowX: "auto" }}>
                    <table
                      className="conflict-diff-table"
                      style={{
                        width: "100%",
                        borderCollapse: "collapse",
                        fontSize: "13px",
                      }}
                    >
                      <thead>
                        <tr>
                          <th style={sTh}>Field</th>
                          <th
                            style={{
                              ...sTh,
                              borderLeft: `2px solid ${C.warning}`,
                            }}
                          >
                            <span style={{ color: C.warning }}>●</span> Local
                          </th>
                          <th
                            style={{
                              ...sTh,
                              borderLeft: `2px solid ${C.accent}`,
                            }}
                          >
                            <span style={{ color: C.accent }}>●</span> Remote
                          </th>
                        </tr>
                      </thead>
                      <tbody>
                        {fields.map((field) => {
                          const isDiff =
                            conflict.diff_fields.includes(field);
                          const localVal = conflict.local_version[field];
                          const remoteVal =
                            conflict.remote_version[field];

                          return (
                            <tr
                              key={field}
                              style={{
                                borderBottom: `1px solid ${C.border}40`,
                              }}
                            >
                              {/* Field name */}
                              <td
                                style={{
                                  ...sTd,
                                  fontWeight: isDiff ? 600 : 400,
                                  color: isDiff
                                    ? C.warning
                                    : C.textSecondary,
                                  backgroundColor: isDiff
                                    ? C.warningTint
                                    : "transparent",
                                }}
                              >
                                {prettyField(field)}
                                {isDiff && (
                                  <span
                                    title="This field differs between versions"
                                    style={{
                                      marginLeft: "6px",
                                      fontSize: "10px",
                                      color: C.warning,
                                    }}
                                  >
                                    ⚠
                                  </span>
                                )}
                              </td>

                              {/* Local value */}
                              <td
                                style={{
                                  ...sTd,
                                  backgroundColor: isDiff
                                    ? C.warningTint
                                    : "transparent",
                                  borderLeft: `2px solid ${
                                    isDiff ? C.warning : "transparent"
                                  }`,
                                  color: C.text,
                                  fontFamily: "monospace",
                                  fontSize: "12px",
                                }}
                              >
                                {formatValue(field, localVal)}
                              </td>

                              {/* Remote value */}
                              <td
                                style={{
                                  ...sTd,
                                  backgroundColor: isDiff
                                    ? C.warningTint
                                    : "transparent",
                                  borderLeft: `2px solid ${
                                    isDiff ? C.accent : "transparent"
                                  }`,
                                  color: C.text,
                                  fontFamily: "monospace",
                                  fontSize: "12px",
                                }}
                              >
                                {formatValue(field, remoteVal)}
                              </td>
                            </tr>
                          );
                        })}
                      </tbody>
                    </table>
                  </div>

                  {/* ── Resolution buttons ──────────────────────── */}
                  <div
                    className="conflict-resolve-actions"
                    style={{
                      display: "flex",
                      gap: "10px",
                      marginTop: "16px",
                      flexWrap: "wrap",
                    }}
                  >
                    <button
                      onClick={() => handleResolve(conflict.id, "local")}
                      disabled={isResolving}
                      style={{
                        ...sResolveBtn,
                        backgroundColor: `${C.accent}20`,
                        borderColor: C.accent,
                        color: C.accent,
                        opacity: isResolving ? 0.5 : 1,
                        cursor: isResolving ? "default" : "pointer",
                      }}
                      title="Discard remote changes, keep your local version"
                    >
                      ← Keep Local
                    </button>
                    <button
                      onClick={() => handleResolve(conflict.id, "remote")}
                      disabled={isResolving}
                      style={{
                        ...sResolveBtn,
                        backgroundColor: `${C.accent}20`,
                        borderColor: C.accent,
                        color: C.accent,
                        opacity: isResolving ? 0.5 : 1,
                        cursor: isResolving ? "default" : "pointer",
                      }}
                      title="Discard local changes, keep the remote version"
                    >
                      Keep Remote →
                    </button>
                    <button
                      onClick={() => handleResolve(conflict.id, "merged")}
                      disabled={isResolving}
                      style={{
                        ...sResolveBtn,
                        backgroundColor: C.successBg,
                        borderColor: C.success,
                        color: C.success,
                        opacity: isResolving ? 0.5 : 1,
                        cursor: isResolving ? "default" : "pointer",
                      }}
                      title="Merge non-conflicting field changes from both versions"
                    >
                      ⇄ Keep Both (Merge)
                    </button>
                  </div>
                </div>
              )}
            </div>
          );
        })}
      </div>

      {/* ── Toast notifications ─────────────────────────────────── */}
      <ToastStack toasts={toasts} />
    </>
  );
}

export default ConflictResolver;
