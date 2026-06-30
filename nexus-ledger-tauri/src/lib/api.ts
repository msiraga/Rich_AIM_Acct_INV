/**
 * Centralized API client for NexusLedger.
 *
 * Automatically attaches a `Bearer <access_token>` Authorization header to
 * every request.  On a 401 response it transparently attempts a token refresh
 * via `POST /api/auth/refresh` and retries the original request once.  If the
 * refresh fails the stored credentials are cleared and the user is redirected
 * to `/login`.
 */

export const API_BASE = "http://localhost:4000";

const ACCESS_TOKEN_KEY = "nexus_access_token";
const REFRESH_TOKEN_KEY = "nexus_refresh_token";
const USER_KEY = "nexus_user";

/* ------------------------------------------------------------------ */
/*  Storage helpers                                                    */
/* ------------------------------------------------------------------ */

function getAccessToken(): string | null {
  return localStorage.getItem(ACCESS_TOKEN_KEY);
}

function getRefreshToken(): string | null {
  return localStorage.getItem(REFRESH_TOKEN_KEY);
}

export function clearAuthStorage(): void {
  localStorage.removeItem(ACCESS_TOKEN_KEY);
  localStorage.removeItem(REFRESH_TOKEN_KEY);
  localStorage.removeItem(USER_KEY);
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

  // --- 401 → attempt refresh + retry once ---
  if (response.status === 401) {
    const refreshedToken = await refreshAccessToken();
    if (refreshedToken) {
      accessToken = refreshedToken;
      response = await doFetch(accessToken);
    }

    if (response.status === 401) {
      // Still unauthorized after refresh attempt.
      redirectToLogin();
      throw new Error("Session expired. Please log in again.");
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
