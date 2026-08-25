import { useEffect, useState } from "react";
import { api, type StatusResponse } from "./api/tauri";
import { Home } from "./pages/Home";
import { Nodes } from "./pages/Nodes";
import { Subscriptions } from "./pages/Subscriptions";
import { Rules } from "./pages/Rules";
import { Logs } from "./pages/Logs";
import { Settings } from "./pages/Settings";
import "./App.css";

type Tab = "home" | "nodes" | "subs" | "rules" | "logs" | "settings";

function App() {
  const [tab, setTab] = useState<Tab>("home");
  const [globalStatus, setGlobalStatus] = useState<StatusResponse | null>(null);

  useEffect(() => {
    let cancelled = false;
    const poll = async () => {
      try {
        const status = await api.getStatus();
        if (!cancelled) setGlobalStatus(status);
      } catch {
        // Background poll; tab pages surface their own errors.
      }
    };
    void poll();
    const id = window.setInterval(() => void poll(), 2000);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, []);

  return (
    <div className="app">
      <header className="top">
        <div>
          <h1>ice-box</h1>
          <p className="tagline">macOS · Windows · sing-box · 订阅管理</p>
        </div>
        <nav className="tabs" aria-label="主导航">
          {(
            [
              ["home", "主页"],
              ["nodes", "节点"],
              ["rules", "规则"],
              ["subs", "订阅"],
              ["logs", "日志"],
              ["settings", "设置"],
            ] as const
          ).map(([id, label]) => (
            <button
              key={id}
              type="button"
              className={tab === id ? "tab active" : "tab"}
              onClick={() => setTab(id)}
            >
              {label}
            </button>
          ))}
        </nav>
      </header>

      {globalStatus?.proxy_recovery_warning && (
        <p className="global-banner error" role="alert">
          {globalStatus.proxy_recovery_warning}
        </p>
      )}

      <main>
        {tab === "home" && <Home onNavigate={setTab} />}
        {tab === "nodes" && <Nodes onNavigate={setTab} />}
        {tab === "subs" && <Subscriptions />}
        {tab === "rules" && <Rules onNavigate={setTab} />}
        {tab === "logs" && <Logs />}
        {tab === "settings" && <Settings />}
      </main>
    </div>
  );
}

export default App;
