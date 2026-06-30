import { useState, useEffect } from "react";
import { Link } from "react-router-dom";
import { apiGet } from "../lib/api";

interface StatusData {
  status: string;
  uptime_seconds: number;
  version: string;
  agents: {
    total: number;
    active: number;
    idle: number;
    error: number;
  };
  tasks: {
    processed: number;
    failed: number;
    in_progress: number;
  };
  health_score: number;
}

function DashboardPage() {
  const [status, setStatus] = useState<StatusData | null>(null);
  const [accounts, setAccounts] = useState<{ id: string; number: string; name: string; balance: string; type: string }[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    Promise.all([
      apiGet<{ success: boolean; data: StatusData }>("/api/v1/status"),
      apiGet<{ success: boolean; data: typeof accounts }>("/api/v1/accounts"),
    ])
      .then(([statusRes, accountsRes]) => {
        if (statusRes.success) setStatus(statusRes.data);
        if (accountsRes.success) setAccounts(accountsRes.data);
      })
      .catch((err) => setError(err.message))
      .finally(() => setLoading(false));
  }, []);

  if (loading) return <div className="page-loading">Loading dashboard...</div>;
  if (error) return <div className="page-error">Error: {error}</div>;

  const assetAccounts = accounts.filter((a) => a.type === "Asset");
  const totalCash = assetAccounts.reduce((sum, a) => sum + parseFloat(a.balance || "0"), 0);

  return (
    <div className="page">
      <h1>Dashboard</h1>

      <div className="dashboard-grid">
        <div className="card">
          <h3>💰 Cash Position</h3>
          <div className="big-number">${totalCash.toFixed(2)}</div>
          <div className="text-muted">Across {assetAccounts.length} asset accounts</div>
        </div>

        <div className="card">
          <h3>🤖 System Health</h3>
          <div className="big-number">{((status?.health_score ?? 0) * 100).toFixed(0)}%</div>
          <div className="text-muted">
            {status?.agents.active ?? 0} active / {status?.agents.total ?? 0} agents
          </div>
        </div>

        <div className="card">
          <h3>📋 Tasks</h3>
          <div className="big-number">{status?.tasks.processed ?? 0}</div>
          <div className="text-muted">
            {status?.tasks.failed ?? 0} failed · {status?.tasks.in_progress ?? 0} in progress
          </div>
        </div>

        <div className="card">
          <h3>⏱️ Uptime</h3>
          <div className="big-number">{Math.floor((status?.uptime_seconds ?? 0) / 60)}m</div>
          <div className="text-muted">v{status?.version ?? "0.1.0"}</div>
        </div>
      </div>

      <div className="card">
        <h3>Quick Actions</h3>
        <div className="quick-actions">
          <Link to="/journal" className="btn btn-primary">
            📝 New Journal Entry
          </Link>
          <Link to="/invoices" className="btn btn-secondary">
            📄 Create Invoice
          </Link>
          <Link to="/accounts" className="btn btn-secondary">
            📊 View Accounts
          </Link>
          <Link to="/transactions" className="btn btn-secondary">
            📋 Transaction Ledger
          </Link>
        </div>
      </div>

      <div className="card">
        <h3>Asset Accounts</h3>
        <table className="table">
          <thead>
            <tr>
              <th>Code</th>
              <th>Name</th>
              <th>Balance</th>
            </tr>
          </thead>
          <tbody>
            {assetAccounts.slice(0, 5).map((acc) => (
              <tr key={acc.id}>
                <td>{acc.number}</td>
                <td>{acc.name}</td>
                <td>${parseFloat(acc.balance).toFixed(2)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

export default DashboardPage;
