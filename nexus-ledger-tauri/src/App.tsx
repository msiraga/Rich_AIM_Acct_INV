import { Routes, Route } from "react-router-dom";
import Layout from "./components/Layout";
import ChatSidebar from "./components/ChatSidebar";
import ErrorBoundary from "./components/ErrorBoundary";
import DashboardPage from "./pages/DashboardPage";
import AccountsPage from "./pages/AccountsPage";
import TransactionsPage from "./pages/TransactionsPage";
import InvoicesPage from "./pages/InvoicesPage";
import JournalEntryPage from "./pages/JournalEntryPage";

function App() {
  return (
    <ErrorBoundary>
      <Routes>
        <Route element={<Layout />}>
          <Route path="/" element={<DashboardPage />} />
          <Route path="/accounts" element={<AccountsPage />} />
          <Route path="/transactions" element={<TransactionsPage />} />
          <Route path="/invoices" element={<InvoicesPage />} />
          <Route path="/journal" element={<JournalEntryPage />} />
        </Route>
      </Routes>
      <ChatSidebar />
    </ErrorBoundary>
  );
}

export default App;
