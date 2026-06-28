import { useState, useEffect } from "react";

interface Account {
  id: string;
  name: string;
  account_type: string;
  balance: number;
}

interface Invoice {
  id: string;
  customer: string;
  amount: number;
  description: string;
  status: string;
}

function App() {
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [invoices, setInvoices] = useState<Invoice[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const fetchData = async () => {
      try {
        setLoading(true);
        const [accountsRes, invoicesRes] = await Promise.all([
          fetch("http://localhost:4000/api/accounts"),
          fetch("http://localhost:4000/api/invoices"),
        ]);
        const accountsData = await accountsRes.json();
        const invoicesData = await invoicesRes.json();
        setAccounts(accountsData);
        setInvoices(invoicesData);
      } catch (err) {
        setError(err instanceof Error ? err.message : "Unknown error");
      } finally {
        setLoading(false);
      }
    };
    fetchData();
  }, []);

  const handleCreateInvoice = async () => {
    try {
      const response = await fetch("http://localhost:4000/api/invoices", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          customer: "Test Customer",
          amount: 100.0,
          description: "Test Invoice",
        }),
      });
      const newInvoice = await response.json();
      setInvoices([...invoices, newInvoice]);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Unknown error");
    }
  };

  if (loading) return <div className="dashboard">Loading...</div>;
  if (error) return <div className="dashboard">Error: {error}</div>;

  return (
    <div className="dashboard">
      <h1>NexusLedger</h1>
      <button onClick={handleCreateInvoice}>Create Test Invoice</button>

      <div className="card">
        <h2>Accounts</h2>
        <table className="table">
          <thead>
            <tr>
              <th>Name</th>
              <th>Type</th>
              <th>Balance</th>
            </tr>
          </thead>
          <tbody>
            {accounts.map((account) => (
              <tr key={account.id}>
                <td>{account.name}</td>
                <td>{account.account_type}</td>
                <td>{account.balance}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <div className="card">
        <h2>Invoices</h2>
        <table className="table">
          <thead>
            <tr>
              <th>Customer</th>
              <th>Amount</th>
              <th>Status</th>
            </tr>
          </thead>
          <tbody>
            {invoices.map((invoice) => (
              <tr key={invoice.id}>
                <td>{invoice.customer}</td>
                <td>{invoice.amount}</td>
                <td>{invoice.status}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

export default App;
