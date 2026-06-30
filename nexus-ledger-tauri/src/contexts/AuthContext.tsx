import { createContext, useContext, useState, useCallback, useEffect, ReactNode } from "react";
import { API_BASE } from "../lib/api";

interface User {
  user_id: string;
  username: string;
  role: string;
}

interface AuthState {
  user: User | null;
  accessToken: string | null;
  refreshToken: string | null;
  isAuthenticated: boolean;
  isLoading: boolean;
}

interface AuthContextType extends AuthState {
  login: (username: string, password: string) => Promise<void>;
  register: (username: string, email: string, password: string, displayName?: string) => Promise<void>;
  logout: () => void;
  getAccessToken: () => string | null;
}

const AuthContext = createContext<AuthContextType | null>(null);

const TOKEN_KEY = "nexus_access_token";
const REFRESH_KEY = "nexus_refresh_token";
const USER_KEY = "nexus_user";

function loadStoredAuth(): AuthState {
  const accessToken = localStorage.getItem(TOKEN_KEY);
  const refreshToken = localStorage.getItem(REFRESH_KEY);
  const userStr = localStorage.getItem(USER_KEY);
  const user = userStr ? JSON.parse(userStr) : null;
  return {
    user,
    accessToken,
    refreshToken,
    isAuthenticated: !!accessToken && !!user,
    isLoading: true,
  };
}

export function AuthProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<AuthState>(() => ({
    ...loadStoredAuth(),
    isLoading: true,
  }));

  const login = useCallback(async (username: string, password: string) => {
    const res = await fetch(`${API_BASE}/api/auth/login`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ username, password }),
    });
    const json = await res.json();
    if (!json.success) throw new Error(json.error || "Login failed");

    const { user_id, username: uname, role, access_token, refresh_token } = json.data;
    const user: User = { user_id, username: uname, role };
    localStorage.setItem(TOKEN_KEY, access_token);
    localStorage.setItem(REFRESH_KEY, refresh_token);
    localStorage.setItem(USER_KEY, JSON.stringify(user));
    setState({ user, accessToken: access_token, refreshToken: refresh_token, isAuthenticated: true, isLoading: false });
  }, []);

  const register = useCallback(async (username: string, email: string, password: string, displayName?: string) => {
    const res = await fetch(`${API_BASE}/api/auth/register`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ username, email, password, display_name: displayName }),
    });
    const json = await res.json();
    if (!json.success) throw new Error(json.error || "Registration failed");

    const { user_id, username: uname, role, access_token, refresh_token } = json.data;
    const user: User = { user_id, username: uname, role };
    localStorage.setItem(TOKEN_KEY, access_token);
    localStorage.setItem(REFRESH_KEY, refresh_token);
    localStorage.setItem(USER_KEY, JSON.stringify(user));
    setState({ user, accessToken: access_token, refreshToken: refresh_token, isAuthenticated: true, isLoading: false });
  }, []);

  const logout = useCallback(() => {
    localStorage.removeItem(TOKEN_KEY);
    localStorage.removeItem(REFRESH_KEY);
    localStorage.removeItem(USER_KEY);
    setState({ user: null, accessToken: null, refreshToken: null, isAuthenticated: false, isLoading: false });
  }, []);

  const getAccessToken = useCallback(() => state.accessToken, [state.accessToken]);

  // Try refreshing token on mount
  useEffect(() => {
    const refresh = async () => {
      const stored = loadStoredAuth();
      if (!stored.refreshToken || stored.isAuthenticated) {
        setState((prev) => ({ ...prev, isLoading: false }));
        return;
      }
      try {
        const res = await fetch(`${API_BASE}/api/auth/refresh`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ refresh_token: stored.refreshToken }),
        });
        const json = await res.json();
        if (json.success) {
          localStorage.setItem(TOKEN_KEY, json.data.access_token);
          if (json.data.refresh_token) {
            localStorage.setItem(REFRESH_KEY, json.data.refresh_token);
          }
          setState((prev) => ({
            ...prev,
            accessToken: json.data.access_token,
            isAuthenticated: true,
            user: stored.user,
            isLoading: false,
          }));
        } else {
          logout();
        }
      } catch {
        logout();
      }
    };
    refresh();
  }, [logout]);

  return (
    <AuthContext.Provider value={{ ...state, login, register, logout, getAccessToken }}>
      {children}
    </AuthContext.Provider>
  );
}

export function useAuth() {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error("useAuth must be used within AuthProvider");
  return ctx;
}
