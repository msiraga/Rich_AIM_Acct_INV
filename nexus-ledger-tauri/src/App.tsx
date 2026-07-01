import { Routes, Route } from "react-router-dom";
import Layout from "./components/Layout";
import ChatSidebar from "./components/ChatSidebar";
import ErrorBoundary from "./components/ErrorBoundary";
import ProtectedRoute from "./components/ProtectedRoute";
import SyncStatus from "./components/SyncStatus";
import DashboardPage from "./pages/DashboardPage";
import AccountsPage from "./pages/AccountsPage";
import TransactionsPage from "./pages/TransactionsPage";
import InvoicesPage from "./pages/InvoicesPage";
import JournalEntryPage from "./pages/JournalEntryPage";
import DocumentsPage from "./pages/DocumentsPage";
import LoginPage from "./pages/LoginPage";
import RegisterPage from "./pages/RegisterPage";

function App() {
  return (
    <ErrorBoundary>
      <Routes>
        {/* Public auth routes */}
        <Route path="/login" element={<LoginPage />} />
        <Route path="/register" element={<RegisterPage />} />

        {/* Protected app routes */}
        <Route element={<ProtectedRoute />}>
          <Route
            element={
              <>
                <Layout />
                <ChatSidebar />
                <SyncStatus />
              </>
            }
          >
            <Route path="/" element={<DashboardPage />} />
            <Route path="/accounts" element={<AccountsPage />} />
            <Route path="/transactions" element={<TransactionsPage />} />
            <Route path="/invoices" element={<InvoicesPage />} />
            <Route path="/documents" element={<DocumentsPage />} />
            <Route path="/journal" element={<JournalEntryPage />} />
          </Route>
        </Route>
      </Routes>
    </ErrorBoundary>
  );
}

export default App;
