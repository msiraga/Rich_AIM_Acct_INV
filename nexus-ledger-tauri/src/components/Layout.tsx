import { Outlet, Link, useLocation, useNavigate } from "react-router-dom";
import { useAuth } from "../contexts/AuthContext";

function Layout() {
  const location = useLocation();
  const { user, logout } = useAuth();
  const navigate = useNavigate();

  const canWrite = user && (user.role === "user" || user.role === "admin");

  const navItems = [
    { path: "/", label: "Dashboard" },
    { path: "/accounts", label: "Accounts" },
    { path: "/transactions", label: "Transactions" },
    { path: "/invoices", label: "Invoices" },
    ...(canWrite ? [{ path: "/journal", label: "New Entry" }] : []),
  ];

  const handleLogout = () => {
    logout();
    navigate("/login");
  };

  return (
    <div className="app-layout">
      <nav className="sidebar">
        <div className="sidebar-header">
          <h2>NexusLedger</h2>
        </div>
        {user && (
          <div className="sidebar-user">
            <span className="user-name">{user.username}</span>
            <span className={`user-role-badge badge-${user.role}`}>{user.role}</span>
          </div>
        )}
        <ul className="nav-list">
          {navItems.map((item) => (
            <li key={item.path}>
              <Link
                to={item.path}
                className={`nav-link ${location.pathname === item.path ? "active" : ""}`}
              >
                {item.label}
              </Link>
            </li>
          ))}
        </ul>
        <div className="sidebar-footer">
          <button onClick={handleLogout} className="btn-secondary btn-small" style={{ width: "100%" }}>
            Sign Out
          </button>
        </div>
      </nav>
      <main className="main-content">
        <Outlet />
      </main>
    </div>
  );
}

export default Layout;
