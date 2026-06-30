import { useState, useEffect } from "react";
import { apiGet, apiPost } from "../lib/api";

interface Account {
  id: string;
  number: string;
  name: string;
  type: string;
}

interface EntryRow {
  account_id: string;
  amount: string;
  entry_type: "Debit" | "Credit";
  description: string;
}

function JournalEntryPage() {
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [description, setDescription] = useState("");
  const [entries, setEntries] = useState<EntryRow[]>([
    { account_id: "", amount: "", entry_type: "Debit", description: "" },
    { account_id: "", amount: "", entry_type: "Credit", description: "" },
  ]);
  const [submitting, setSubmitting] = useState(false);
  const [result, setResult] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    apiGet<{ success: boolean; data: Account[] }>("/api/v1/accounts")
      .then((res) => {
        if (res.success) setAccounts(res.data);
      })
      .catch(() => {});
  }, []);

  const addEntry = () => {
    setEntries([...entries, { account_id: "", amount: "", entry_type: "Debit", description: "" }]);
  };

  const removeEntry = (index: number) => {
    if (entries.length <= 2) return;
    setEntries(entries.filter((_, i) => i !== index));
  };

  const updateEntry = (index: number, field: keyof EntryRow, value: string) => {
    const updated = [...entries];
    updated[index] = { ...updated[index], [field]: value };
    setEntries(updated);
  };

  const totalDebits = entries
    .filter((e) => e.entry_type === "Debit")
    .reduce((sum, e) => sum + parseFloat(e.amount || "0"), 0);
  const totalCredits = entries
    .filter((e) => e.entry_type === "Credit")
    .reduce((sum, e) => sum + parseFloat(e.amount || "0"), 0);
  const balanced = Math.abs(totalDebits - totalCredits) < 0.001;

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    setResult(null);

    if (!balanced) {
      setError(`Not balanced! Debits: $${totalDebits.toFixed(2)} ≠ Credits: $${totalCredits.toFixed(2)}`);
      return;
    }

    setSubmitting(true);
    try {
      const res = await apiPost<{ success: boolean; data: { number: string }; error?: string }>(
        "/api/v1/transactions",
        {
          description,
          entries: entries.map((e) => ({
            account_id: e.account_id,
            amount: e.amount,
            entry_type: e.entry_type.toLowerCase(),
            description: e.description,
          })),
        },
      );
      if (res.success) {
        setResult(`Transaction ${res.data.number} created successfully!`);
        setDescription("");
        setEntries([
          { account_id: "", amount: "", entry_type: "Debit", description: "" },
          { account_id: "", amount: "", entry_type: "Credit", description: "" },
        ]);
      } else {
        setError(res.error || "Failed to create transaction");
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : "Unknown error");
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="page">
      <h1>Journal Entry</h1>

      <form className="form card" onSubmit={handleSubmit}>
        <div className="form-group">
          <label>Description</label>
          <input
            type="text"
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            placeholder="e.g., Monthly rent payment"
            required
          />
        </div>

        <div className="entries-section">
          <div className="entries-header">
            <h3>Entries</h3>
            <button type="button" className="btn btn-small" onClick={addEntry}>
              + Add Line
            </button>
          </div>

          <table className="table table-compact">
            <thead>
              <tr>
                <th>Account</th>
                <th>Type</th>
                <th>Amount</th>
                <th>Memo</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {entries.map((entry, i) => (
                <tr key={i}>
                  <td>
                    <select
                      value={entry.account_id}
                      onChange={(e) => updateEntry(i, "account_id", e.target.value)}
                      required
                    >
                      <option value="">Select account...</option>
                      {accounts.map((acc) => (
                        <option key={acc.id} value={acc.id}>
                          {acc.number} — {acc.name}
                        </option>
                      ))}
                    </select>
                  </td>
                  <td>
                    <select
                      value={entry.entry_type}
                      onChange={(e) => updateEntry(i, "entry_type", e.target.value as "Debit" | "Credit")}
                    >
                      <option value="Debit">Debit</option>
                      <option value="Credit">Credit</option>
                    </select>
                  </td>
                  <td>
                    <input
                      type="number"
                      step="0.01"
                      value={entry.amount}
                      onChange={(e) => updateEntry(i, "amount", e.target.value)}
                      placeholder="0.00"
                      required
                    />
                  </td>
                  <td>
                    <input
                      type="text"
                      value={entry.description}
                      onChange={(e) => updateEntry(i, "description", e.target.value)}
                      placeholder="Memo"
                    />
                  </td>
                  <td>
                    <button type="button" className="btn btn-small btn-danger" onClick={() => removeEntry(i)}>
                      ✕
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>

        <div className={`balance-check ${balanced ? "balanced" : "unbalanced"}`}>
          <span>Debits: ${totalDebits.toFixed(2)}</span>
          <span>Credits: ${totalCredits.toFixed(2)}</span>
          <span className={balanced ? "text-positive" : "text-negative"}>
            {balanced ? "✓ Balanced" : `✗ Difference: $${Math.abs(totalDebits - totalCredits).toFixed(2)}`}
          </span>
        </div>

        {error && <div className="form-error">{error}</div>}
        {result && <div className="form-success">{result}</div>}

        <button type="submit" className="btn btn-primary btn-large" disabled={submitting || !balanced}>
          {submitting ? "Posting..." : "Post Journal Entry"}
        </button>
      </form>
    </div>
  );
}

export default JournalEntryPage;
