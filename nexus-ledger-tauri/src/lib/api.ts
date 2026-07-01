/**
 * Centralized API client for NexusLedger.
 *
 * Automatically attaches a `Bearer <access_token>` Authorization header to
 * every request.  On a 401 response it transparently attempts a token refresh
 * via `POST /api/auth/refresh` and retries the original request once.  If the
 * refresh fails the stored credentials are cleared and the user is redirected
 * to `/login`.
 *
 * CSRF protection uses the double-submit pattern: every POST/PUT/DELETE/PATCH
 * request includes an `X-CSRF-Token` header read from localStorage.  The server
 * can rotate the token at any time via an `X-CSRF-Token` response header,
 * which is captured automatically.  On a 403 with `X-CSRF-Reason: invalid_token`
 * the client fetches a fresh token from `/api/auth/csrf-token` and retries the
 * original request once.
 */

export const API_BASE = "http://localhost:8080";

const ACCESS_TOKEN_KEY = "nexus_access_token";
const REFRESH_TOKEN_KEY = "nexus_refresh_token";
const USER_KEY = "nexus_user";

const CSRF_TOKEN_KEY = "nexus_csrf_token";
const CSRF_REFRESH_URL = "/api/auth/csrf-token";
const WRITE_METHODS = new Set(["POST", "PUT", "DELETE", "PATCH"]);

/* ------------------------------------------------------------------ */
/*  Storage helpers                                                    */
/* ------------------------------------------------------------------ */

function getAccessToken(): string | null {
  return localStorage.getItem(ACCESS_TOKEN_KEY);
}

function getRefreshToken(): string | null {
  return localStorage.getItem(REFRESH_TOKEN_KEY);
}

export function getCsrfToken(): string | null {
  return localStorage.getItem(CSRF_TOKEN_KEY);
}

export function setCsrfToken(token: string): void {
  localStorage.setItem(CSRF_TOKEN_KEY, token);
}

export function clearAuthStorage(): void {
  localStorage.removeItem(ACCESS_TOKEN_KEY);
  localStorage.removeItem(REFRESH_TOKEN_KEY);
  localStorage.removeItem(USER_KEY);
  localStorage.removeItem(CSRF_TOKEN_KEY);
}

function redirectToLogin(): void {
  clearAuthStorage();
  // Guard against server-side or test environments where `window` is absent.
  if (typeof window !== "undefined") {
    window.location.href = "/login";
  }
}

/* ------------------------------------------------------------------ */
/*  Token refresh                                                      */
/* ------------------------------------------------------------------ */

let refreshPromise: Promise<string | null> | null = null;

async function doRefresh(): Promise<string | null> {
  const refreshToken = getRefreshToken();
  if (!refreshToken) {
    return null;
  }

  const res = await fetch(`${API_BASE}/api/auth/refresh`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ refresh_token: refreshToken }),
  });

  // The refresh response may also issue/rotate a CSRF token.
  captureCsrfToken(res);

  if (!res.ok) {
    return null;
  }

  const json = await res.json();
  if (!json.success) {
    return null;
  }

  const { access_token, refresh_token: newRefreshToken } = json.data;
  localStorage.setItem(ACCESS_TOKEN_KEY, access_token);
  if (newRefreshToken) {
    localStorage.setItem(REFRESH_TOKEN_KEY, newRefreshToken);
  }
  return access_token as string;
}

/**
 * Returns a promise that resolves with a fresh access token.
 * Concurrent callers share the same in-flight refresh request.
 */
function refreshAccessToken(): Promise<string | null> {
  if (!refreshPromise) {
    refreshPromise = doRefresh().finally(() => {
      refreshPromise = null;
    });
  }
  return refreshPromise;
}

/* ------------------------------------------------------------------ */
/*  CSRF token                                                         */
/* ------------------------------------------------------------------ */

/**
 * Capture a CSRF token from a response header if the server provided one.
 * Called on every response so the server can rotate tokens at will.
 */
function captureCsrfToken(response: Response): void {
  const token = response.headers.get("X-CSRF-Token");
  if (token) {
    setCsrfToken(token);
  }
}

let csrfRefreshPromise: Promise<string | null> | null = null;

async function doCsrfRefresh(): Promise<string | null> {
  const res = await fetch(`${API_BASE}${CSRF_REFRESH_URL}`, {
    method: "GET",
    headers: { "Content-Type": "application/json" },
  });

  if (!res.ok) {
    return null;
  }

  // The server may return the token in a response header or in the JSON body.
  const headerToken = res.headers.get("X-CSRF-Token");
  if (headerToken) {
    setCsrfToken(headerToken);
    return headerToken;
  }

  try {
    const json = await res.json();
    if (json.success && json.data?.csrf_token) {
      setCsrfToken(json.data.csrf_token);
      return json.data.csrf_token as string;
    }
  } catch {
    // Response wasn't JSON; token may have already been captured from header.
  }

  return getCsrfToken();
}

/**
 * Returns a promise that resolves with a fresh CSRF token.
 * Concurrent callers share the same in-flight refresh request.
 */
function refreshCsrfToken(): Promise<string | null> {
  if (!csrfRefreshPromise) {
    csrfRefreshPromise = doCsrfRefresh().finally(() => {
      csrfRefreshPromise = null;
    });
  }
  return csrfRefreshPromise;
}

/* ------------------------------------------------------------------ */
/*  Core fetchWithAuth                                                  */
/* ------------------------------------------------------------------ */

type FetchOptions = Omit<RequestInit, "body"> & { body?: unknown };

async function fetchWithAuth<T = unknown>(
  url: string,
  options: FetchOptions = {},
): Promise<T> {
  const { body, headers, ...rest } = options;

  const buildHeaders = (token: string | null): HeadersInit => {
    const h: Record<string, string> = {
      "Content-Type": "application/json",
      ...((headers as Record<string, string>) || {}),
    };
    if (token) {
      h["Authorization"] = `Bearer ${token}`;
    }
    // Attach CSRF token to state-changing requests (double-submit pattern).
    if (WRITE_METHODS.has((rest.method || "GET").toUpperCase())) {
      const csrf = getCsrfToken();
      if (csrf) {
        h["X-CSRF-Token"] = csrf;
      }
    }
    return h;
  };

  const serializeBody = (): string | undefined => {
    if (body === undefined || body === null) return undefined;
    return typeof body === "string" ? body : JSON.stringify(body);
  };

  const doFetch = (token: string | null): Promise<Response> =>
    fetch(`${API_BASE}${url}`, {
      ...rest,
      headers: buildHeaders(token),
      body: serializeBody(),
    });

  let accessToken = getAccessToken();
  let response = await doFetch(accessToken);

  // Capture CSRF token from any response (server may rotate it).
  captureCsrfToken(response);

  // --- 401 → attempt auth refresh + retry once ---
  if (response.status === 401) {
    const refreshedToken = await refreshAccessToken();
    if (refreshedToken) {
      accessToken = refreshedToken;
      response = await doFetch(accessToken);
      captureCsrfToken(response);
    }

    if (response.status === 401) {
      // Still unauthorized after refresh attempt.
      redirectToLogin();
      throw new Error("Session expired. Please log in again.");
    }
  }

  // --- 403 invalid CSRF → refresh CSRF token + retry once ---
  if (
    response.status === 403 &&
    response.headers.get("X-CSRF-Reason") === "invalid_token"
  ) {
    const refreshedCsrf = await refreshCsrfToken();
    if (refreshedCsrf) {
      response = await doFetch(accessToken);
      captureCsrfToken(response);
    }
  }

  return response.json() as Promise<T>;
}

/* ------------------------------------------------------------------ */
/*  Convenience helpers                                                 */
/* ------------------------------------------------------------------ */

export function apiGet<T = unknown>(url: string, options?: FetchOptions): Promise<T> {
  return fetchWithAuth<T>(url, { ...options, method: "GET" });
}

export function apiPost<T = unknown>(url: string, body?: unknown, options?: FetchOptions): Promise<T> {
  return fetchWithAuth<T>(url, { ...options, method: "POST", body });
}

export function apiPut<T = unknown>(url: string, body?: unknown, options?: FetchOptions): Promise<T> {
  return fetchWithAuth<T>(url, { ...options, method: "PUT", body });
}

export function apiDelete<T = unknown>(url: string, options?: FetchOptions): Promise<T> {
  return fetchWithAuth<T>(url, { ...options, method: "DELETE" });
}

export { fetchWithAuth };
