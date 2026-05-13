import type { ButtonHTMLAttributes, ReactNode } from "react";
import type { BuildStateLabel, View } from "../hooks/useBuildLauncher";
import type { LogEvent, ToolProbe } from "../types";

export function Icon({ name }: { name: "home" | "workflow" | "logs" | "artifact" | "settings" | "play" | "folder" | "save" | "copy" | "trash" | "star" | "refresh" }) {
  const common = { width: 16, height: 16, viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", strokeWidth: 2, strokeLinecap: "round", strokeLinejoin: "round" } as const;
  const paths = {
    home: <><path d="m3 11 9-8 9 8" /><path d="M5 10v10h14V10" /><path d="M9 20v-6h6v6" /></>,
    workflow: <><path d="M6 3v6" /><path d="M6 15v6" /><path d="M18 3v6" /><path d="M18 15v6" /><path d="M6 9h12" /><path d="M6 15h12" /><circle cx="6" cy="12" r="3" /><circle cx="18" cy="12" r="3" /></>,
    logs: <><path d="M8 6h13" /><path d="M8 12h13" /><path d="M8 18h13" /><path d="M3 6h.01" /><path d="M3 12h.01" /><path d="M3 18h.01" /></>,
    artifact: <><path d="M21 8v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8" /><path d="M7 8V3h10v5" /><path d="M3 8h18" /><path d="M9 13h6" /></>,
    settings: <><path d="M12 15.5a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7Z" /><path d="M19.4 15a1.8 1.8 0 0 0 .36 1.98l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06A1.8 1.8 0 0 0 15 19.4a1.8 1.8 0 0 0-1 .6 1.8 1.8 0 0 0-.4 1.1V21a2 2 0 0 1-4 0v-.09A1.8 1.8 0 0 0 8.6 19.4a1.8 1.8 0 0 0-1.98.36l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.8 1.8 0 0 0 4.6 15a1.8 1.8 0 0 0-.6-1 1.8 1.8 0 0 0-1.1-.4H3a2 2 0 0 1 0-4h.09A1.8 1.8 0 0 0 4.6 8.6a1.8 1.8 0 0 0-.36-1.98l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.8 1.8 0 0 0 9 4.6a1.8 1.8 0 0 0 1-.6 1.8 1.8 0 0 0 .4-1.1V3a2 2 0 0 1 4 0v.09A1.8 1.8 0 0 0 15.4 4.6a1.8 1.8 0 0 0 1.98-.36l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.8 1.8 0 0 0 19.4 9c.31.38.72.6 1.1.6H21a2 2 0 0 1 0 4h-.09a1.8 1.8 0 0 0-1.51 1.4Z" /></>,
    play: <path d="m8 5 11 7-11 7V5Z" />,
    folder: <><path d="M3 7a2 2 0 0 1 2-2h5l2 2h7a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z" /></>,
    save: <><path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2Z" /><path d="M17 21v-8H7v8" /><path d="M7 3v5h8" /></>,
    copy: <><rect x="9" y="9" width="11" height="11" rx="2" /><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" /></>,
    trash: <><path d="M3 6h18" /><path d="M8 6V4h8v2" /><path d="M19 6l-1 14H6L5 6" /></>,
    star: <path d="m12 3 2.8 5.7 6.2.9-4.5 4.4 1.1 6.2L12 17.3l-5.6 2.9 1.1-6.2L3 9.6l6.2-.9L12 3Z" />,
    refresh: <><path d="M21 12a9 9 0 0 1-15.2 6.5" /><path d="M3 12A9 9 0 0 1 18.2 5.5" /><path d="M18 2v4h4" /><path d="M6 22v-4H2" /></>,
  }[name];
  return <svg {...common}>{paths}</svg>;
}

export function Panel({ title, kicker, actions, children }: { title: string; kicker?: string; actions?: ReactNode; children: ReactNode }) {
  return (
    <section className="panel">
      <div className="panel-header">
        <div>
          {kicker && <p className="panel-kicker">{kicker}</p>}
          <h2>{title}</h2>
        </div>
        {actions && <div className="panel-actions">{actions}</div>}
      </div>
      {children}
    </section>
  );
}

export function Field({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <label className="field-label">
      <span>{label}</span>
      {children}
      {hint && <small>{hint}</small>}
    </label>
  );
}

export function Button({ children, variant = "primary", icon, ...props }: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: "primary" | "secondary" | "danger" | "ghost"; icon?: ReactNode }) {
  return (
    <button className={`button ${variant}`} {...props}>
      {icon}
      <span>{children}</span>
    </button>
  );
}

export function ReadinessBadge({ label, ready }: { label: string; ready: boolean }) {
  return <span className={`readiness-badge ${ready ? "ready" : "missing"}`}>{label}</span>;
}

export function ToolRow({ label, probe }: { label: string; probe?: ToolProbe }) {
  const available = probe?.available ?? false;
  return (
    <div className="tool-row">
      <div>
        <h3>{label}</h3>
        <p>{probe?.message ?? "Checking..."}</p>
        {probe?.path && <small>{probe.path}</small>}
      </div>
      <ReadinessBadge label={available ? "Ready" : "Missing"} ready={available} />
    </div>
  );
}

export function LogLine({ log }: { log: LogEvent }) {
  return <div className={`log-line ${log.level}`}>{log.message}</div>;
}

export function BuildStatePill({ state }: { state: BuildStateLabel }) {
  const label = {
    idle: "Ready",
    running: "Running",
    success: "Complete",
    failed: "Failed",
    cancelled: "Cancelled",
  }[state];
  return <span className={`build-pill ${state}`}>{label}</span>;
}

export const navItems: Array<{ id: View; label: string; icon: Parameters<typeof Icon>[0]["name"] }> = [
  { id: "home", label: "Home", icon: "home" },
  { id: "workflows", label: "Workflows", icon: "workflow" },
  { id: "logs", label: "Logs", icon: "logs" },
  { id: "artifacts", label: "Artifacts", icon: "artifact" },
  { id: "settings", label: "Settings", icon: "settings" },
];
