import { useState, useEffect } from "react";
import { apiGet } from "../lib/api";

interface Entry {
  account_id: string;
  amount: string;
  entry_type: string;
}

interface Transaction {
  id: string;
  number: string;
  description: string;
  date: string;
  status: string;
  total_amount: string;
  entries: Entry[];
}

interface Pagination {
  total: number;
  limit: number;
  offset: number;
}

function TransactionsPage() {
  const [transactions, setTransactions] = useState<Transaction[]>([]);
  const [pagination, setPagination] = useState<Pagination | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [page, setPage] = useState(0);
  const limit = 20;

  const fetchTransactions = (offset: number) => {
    setLoading(true);
    apiGet<{ success: boolean; data: { data: Transaction[]; pagination: Pagination }; error?: string }>(
      `/api/v1/transactions?limit=${limit}&offset=${offset}`,
    )
      .then((res) => {
        if (res.success) {
          setTransactions(res.data.data);
          setPagination(res.data.pagination);
        } else {
          setError(res.error || "Failed to load transactions");
        }
      })
      .catch((err) => setError(err.message))
      .finally(() => setLoading(false));
  };

  useEffect(() => {
    fetchTransactions(page * limit);
  }, [page]);

  if (loading) return <div className="page-loading">Loading transactions...</div>;
  if (error) return <div className="page-error">Error: {error}</div>;

  return (
    <div className="page">
      <h1>Ledger / Transactions</h1>
      <div className="summary-bar">
        <span>{pagination?.total ?? 0} transactions</span>
      </div>
      <table className="table">
        <thead>
          <tr>
            <th>Number</th>
            <th>Description</th>
            <th>Date</th>
            <th>Amount</th>
            <th>Status</th>
            <th>Entries</th>
          </tr>
        </thead>
        <tbody>
          {transactions.map((txn) => (
            <tr key={txn.id}>
              <td>{txn.number}</td>
              <td>{txn.description}</td>
              <td>{new Date(txn.date).toLocaleDateString()}</td>
              <td>${parseFloat(txn.total_amount).toFixed(2)}</td>
              <td>
                <span className={`badge badge-${txn.status.toLowerCase()}`}>{txn.status}</span>
              </td>
              <td>
                <div className="entry-list">
                  {txn.entries.map((e, i) => (
                    <span key={i} className={`entry-tag entry-${e.entry_type.toLowerCase()}`}>
                      {e.entry_type}: ${parseFloat(e.amount).toFixed(2)}
                    </span>
                  ))}
                </div>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {pagination && (
        <div className="pagination">
          <button disabled={page === 0} onClick={() => setPage(page - 1)}>
            Previous
          </button>
          <span>
            Page {page + 1} of {Math.ceil(pagination.total / limit)}
          </span>
          <button
            disabled={(page + 1) * limit >= pagination.total}
            onClick={() => setPage(page + 1)}
          >
            Next
          </button>
        </div>
      )}
    </div>
  );
}

export default TransactionsPage;
