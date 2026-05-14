import { useEffect, useState } from "react";
import { BuildStatePill, Icon, navItems } from "./components/ui";
import { useBuildLauncher } from "./hooks/useBuildLauncher";
import { ArtifactsView } from "./views/ArtifactsView";
import { HomeView } from "./views/HomeView";
import { LogsView } from "./views/LogsView";
import { SettingsView } from "./views/SettingsView";
import { WorkflowsView } from "./views/WorkflowsView";

export default function App() {
  const launcher = useBuildLauncher();
  const [splashHoldComplete, setSplashHoldComplete] = useState(false);
  const [showSplash, setShowSplash] = useState(true);
  const [splashExiting, setSplashExiting] = useState(false);

  useEffect(() => {
    const timer = window.setTimeout(() => setSplashHoldComplete(true), 1300);
    return () => window.clearTimeout(timer);
  }, []);

  useEffect(() => {
    if (!launcher.appReady || !splashHoldComplete || !showSplash) {
      return;
    }
    setSplashExiting(true);
    const timer = window.setTimeout(() => setShowSplash(false), 260);
    return () => window.clearTimeout(timer);
  }, [launcher.appReady, splashHoldComplete, showSplash]);

  return (
    <main className="app-root">
      {showSplash && (
        <div className={`app-splash ${splashExiting ? "leaving" : ""}`} aria-hidden="true">
          <img className="app-splash-logo" src="/apk-build-launcher-transparent.png" alt="" />
        </div>
      )}

      <div className="desktop-frame">
        <aside className="sidebar">
          <div className="brand-block">
            <img src="/apk-build-launcher-transparent.png" alt="" />
            <div>
              <h1>APK Build Launcher</h1>
              <p>Local Android workflow runner</p>
            </div>
          </div>

          <nav className="side-nav" aria-label="Primary">
            {navItems.map((item) => (
              <button
                key={item.id}
                className={launcher.activeView === item.id ? "active" : ""}
                onClick={() => launcher.setActiveView(item.id)}
              >
                <Icon name={item.icon} />
                <span>{item.label}</span>
              </button>
            ))}
          </nav>

          <div className="sidebar-status">
            <div className="status-title">
              <span>Status</span>
              <BuildStatePill state={launcher.buildState} />
            </div>
            <p>{launcher.status}</p>
          </div>
        </aside>

        <section className="content-shell">
          <header className="topbar">
            <div>
              <p>v2 workspace</p>
              <h2>{launcher.selectedPresetId ? "Preset build" : "Custom build"}</h2>
            </div>
            <div className="topbar-actions">
              <BuildStatePill state={launcher.buildState} />
              <button title="Refresh tool status" onClick={launcher.refreshTools}>
                <Icon name="refresh" />
              </button>
            </div>
          </header>

          <div className="view-stack">
            {launcher.activeView === "home" && <HomeView launcher={launcher} />}
            {launcher.activeView === "workflows" && <WorkflowsView launcher={launcher} />}
            {launcher.activeView === "logs" && <LogsView launcher={launcher} />}
            {launcher.activeView === "artifacts" && <ArtifactsView launcher={launcher} />}
            {launcher.activeView === "settings" && <SettingsView launcher={launcher} />}
          </div>
        </section>
      </div>
    </main>
  );
}
