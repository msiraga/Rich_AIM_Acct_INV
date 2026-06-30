import { useState, useEffect } from "react";

const API = "http://localhost:4000";

interface InvoiceTransaction {
  id: string;
  number: string;
  description: string;
  date: string;
  status: string;
  total_amount: string;
}

function InvoicesPage() {
  const [invoices, setInvoices] = useState<InvoiceTransaction[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showForm, setShowForm] = useState(false);
  const [form, setForm] = useState({ customer_name: "", amount: "", description: "" });
  const [submitting, setSubmitting] = useState(false);

  const fetchInvoices = () => {
    setLoading(true);
    fetch(`${API}/api/v1/invoices`)
      .then((r) => r.json())
      .then((res) => {
        if (res.success) {
          setInvoices(res.data.data);
        } else {
          setError(res.error || "Failed to load invoices");
        }
      })
      .catch((err) => setError(err.message))
      .finally(() => setLoading(false));
  };

  useEffect(() => {
    fetchInvoices();
  }, []);

  const handleCreateInvoice = async (e: React.FormEvent) => {
    e.preventDefault();
    setSubmitting(true);
    try {
      const response = await fetch(`${API}/api/v1/invoices`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          customer_name: form.customer_name,
          items: [{ description: form.description, quantity: 1, unit_price: form.amount }],
        }),
      });
      const res = await response.json();
      if (res.success) {
        setShowForm(false);
        setForm({ customer_name: "", amount: "", description: "" });
        fetchInvoices();
      } else {
        setError(res.error || "Failed to create invoice");
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : "Unknown error");
    } finally {
      setSubmitting(false);
    }
  };

  if (loading) return <div className="page-loading">Loading invoices...</div>;
  if (error) return <div className="page-error">Error: {error}</div>;

  return (
    <div className="page">
      <h1>Invoices</h1>
      <div className="summary-bar">
        <span>{invoices.length} invoices</span>
        <button className="btn btn-primary" onClick={() => setShowForm(!showForm)}>
          {showForm ? "Cancel" : "New Invoice"}
        </button>
      </div>

      {showForm && (
        <form className="form card" onSubmit={handleCreateInvoice}>
          <h3>New Invoice</h3>
          <div className="form-group">
            <label>Customer Name</label>
            <input
              type="text"
              value={form.customer_name}
              onChange={(e) => setForm({ ...form, customer_name: e.target.value })}
              required
            />
          </div>
          <div className="form-group">
            <label>Description</label>
            <input
              type="text"
              value={form.description}
              onChange={(e) => setForm({ ...form, description: e.target.value })}
              required
            />
          </div>
          <div className="form-group">
            <label>Amount ($)</label>
            <input
              type="number"
              step="0.01"
              value={form.amount}
              onChange={(e) => setForm({ ...form, amount: e.target.value })}
              required
            />
          </div>
          <button type="submit" className="btn btn-primary" disabled={submitting}>
            {submitting ? "Creating..." : "Create Invoice"}
          </button>
        </form>
      )}

      <table className="table">
        <thead>
          <tr>
            <th>Number</th>
            <th>Description</th>
            <th>Date</th>
            <th>Amount</th>
            <th>Status</th>
          </tr>
        </thead>
        <tbody>
          {invoices.length === 0 ? (
            <tr>
              <td colSpan={5} className="text-center">
                No invoices yet. Create one above.
              </td>
            </tr>
          ) : (
            invoices.map((inv) => (
              <tr key={inv.id}>
                <td>{inv.number}</td>
                <td>{inv.description}</td>
                <td>{new Date(inv.date).toLocaleDateString()}</td>
                <td>${parseFloat(inv.total_amount).toFixed(2)}</td>
                <td>
                  <span className={`badge badge-${inv.status.toLowerCase()}`}>{inv.status}</span>
                </td>
              </tr>
            ))
          )}
        </tbody>
      </table>
    </div>
  );
}

export default InvoicesPage;
