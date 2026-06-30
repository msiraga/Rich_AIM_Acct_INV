import { useState, useEffect } from "react";
import { Link } from "react-router-dom";
import { apiGet } from "../lib/api";

interface Account {
  id: string;
  number: string;
  name: string;
  type: string;
  balance: string;
  status: string;
}

function AccountsPage() {
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    apiGet<{ success: boolean; data: Account[]; error?: string }>("/api/v1/accounts")
      .then((res) => {
        if (res.success) {
          setAccounts(res.data);
        } else {
          setError(res.error || "Failed to load accounts");
        }
      })
      .catch((err) => setError(err.message))
      .finally(() => setLoading(false));
  }, []);

  if (loading) return <div className="page-loading">Loading accounts...</div>;
  if (error) return <div className="page-error">Error: {error}</div>;

  const totalAssets = accounts
    .filter((a) => a.type === "Asset")
    .reduce((sum, a) => sum + parseFloat(a.balance || "0"), 0);

  return (
    <div className="page">
      <h1>Chart of Accounts</h1>
      <div className="summary-bar">
        <span>Total Assets: ${totalAssets.toFixed(2)}</span>
        <span>{accounts.length} accounts</span>
      </div>
      <table className="table">
        <thead>
          <tr>
            <th>Code</th>
            <th>Name</th>
            <th>Type</th>
            <th>Balance</th>
            <th>Status</th>
          </tr>
        </thead>
        <tbody>
          {accounts.map((acc) => (
            <tr key={acc.id}>
              <td>{acc.number}</td>
              <td>
                <Link to={`/accounts/${acc.id}`}>{acc.name}</Link>
              </td>
              <td>{acc.type}</td>
              <td className={parseFloat(acc.balance) >= 0 ? "text-positive" : "text-negative"}>
                ${parseFloat(acc.balance).toFixed(2)}
              </td>
              <td>{acc.status}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export default AccountsPage;
