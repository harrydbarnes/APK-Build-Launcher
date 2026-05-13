import { listen } from "@tauri-apps/api/event";
import { useEffect, useMemo, useState } from "react";
import { api } from "./tauri";
import type { AppConfig, BuildResult, JobSummary, LogEvent, ShellMode, ToolProbe, ToolStatus, WorkflowSummary } from "./types";

const emptyConfig: AppConfig = {
  defaultRepoFolder: "",
  defaultOutputFolder: "",
  shellMode: "native",
  theme: "system",
};

const tabs = ["Home", "Tools", "Secrets", "Workflows", "Logs", "Settings"] as const;
type Tab = (typeof tabs)[number];

export default function App() {
  const [activeTab, setActiveTab] = useState<Tab>("Home");
  const [config, setConfig] = useState<AppConfig>(emptyConfig);
  const [repoUrl, setRepoUrl] = useState("");
  const [refName, setRefName] = useState("dev");
  const [outputFolder, setOutputFolder] = useState("");
  const [repoPath, setRepoPath] = useState("");
  const [workflows, setWorkflows] = useState<WorkflowSummary[]>([]);
  const [workflowPath, setWorkflowPath] = useState("");
  const [jobId, setJobId] = useState("");
  const [secretDraft, setSecretDraft] = useState<Record<string, string>>({});
  const [savedSecretNames, setSavedSecretNames] = useState<string[]>([]);
  const [logs, setLogs] = useState<LogEvent[]>([]);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState("Ready");
  const [result, setResult] = useState<BuildResult | null>(null);
  const [branches, setBranches] = useState<string[]>([]);
  const [toolStatus, setToolStatus] = useState<ToolStatus | null>(null);
  const [installingTools, setInstallingTools] = useState(false);
  const [loadingBranches, setLoadingBranches] = useState(false);
  const [branchMessage, setBranchMessage] = useState("");
  const [appReady, setAppReady] = useState(false);
  const [splashHoldComplete, setSplashHoldComplete] = useState(false);
  const [showSplash, setShowSplash] = useState(true);
  const [splashExiting, setSplashExiting] = useState(false);

  useEffect(() => {
    api.getConfig().then((loaded) => {
      setConfig(loaded);
      setOutputFolder(loaded.defaultOutputFolder);
    }).catch((error) => setStatus(String(error)))
      .finally(() => setAppReady(true));
    api.getToolStatus().then(setToolStatus).catch(() => undefined);
  }, []);

  useEffect(() => {
    document.documentElement.dataset.theme = config.theme;
  }, [config.theme]);

  useEffect(() => {
    const timer = window.setTimeout(() => setSplashHoldComplete(true), 2200);
    return () => window.clearTimeout(timer);
  }, []);

  useEffect(() => {
    if (!appReady || !splashHoldComplete || !showSplash) {
      return;
    }

    setSplashExiting(true);
    const timer = window.setTimeout(() => setShowSplash(false), 360);
    return () => window.clearTimeout(timer);
  }, [appReady, splashHoldComplete, showSplash]);

  useEffect(() => {
    setBranches([]);
    setBranchMessage("");
  }, [repoUrl]);

  useEffect(() => {
    const unlisten = listen<LogEvent>("build-log", (event) => {
      setLogs((items) => [...items, event.payload]);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const selectedWorkflow = useMemo(
    () => workflows.find((workflow) => workflow.filePath === workflowPath),
    [workflows, workflowPath],
  );

  const selectedJob = useMemo<JobSummary | undefined>(
    () => selectedWorkflow?.jobs.find((job) => job.id === jobId),
    [selectedWorkflow, jobId],
  );

  async function loadBranches() {
    const trimmedRepoUrl = repoUrl.trim();
    if (!trimmedRepoUrl) {
      setBranchMessage("Enter a GitHub repo URL first.");
      return;
    }

    setLoadingBranches(true);
    setBranchMessage("Loading branches...");
    setLogs([]);
    try {
      const loaded = await api.listBranches(trimmedRepoUrl);
      api.getToolStatus().then(setToolStatus).catch(() => undefined);
      setBranches(loaded);
      if (loaded.length && (!refName.trim() || !loaded.includes(refName.trim()))) {
        setRefName(loaded[0]);
      }
      setBranchMessage(loaded.length ? `${loaded.length} branch${loaded.length === 1 ? "" : "es"} loaded.` : "No branches found.");
    } catch (error) {
      const message = String(error);
      setBranchMessage(message);
      setStatus(message);
    } finally {
      setLoadingBranches(false);
    }
  }

  async function prepareAndDetect() {
    setBusy(true);
    setStatus("Preparing tools and repository...");
    setResult(null);
    try {
      const path = await api.prepareRepo(repoUrl.trim(), refName.trim());
      api.getToolStatus().then(setToolStatus).catch(() => undefined);
      setRepoPath(path);
      setStatus("Detecting workflow files...");
      const detected = await api.detectWorkflows(path);
      setWorkflows(detected);
      const firstWorkflow = detected[0];
      setWorkflowPath(firstWorkflow?.filePath ?? "");
      setJobId(firstWorkflow?.jobs[0]?.id ?? "");
      const secrets = await api.getSecrets(repoUrl.trim());
      setSavedSecretNames(secrets.names);
      setActiveTab("Workflows");
      setStatus(detected.length ? "Workflows detected" : "No workflow files found");
    } catch (error) {
      setStatus(String(error));
    } finally {
      setBusy(false);
    }
  }

  async function saveSecrets() {
    setBusy(true);
    try {
      await api.saveSecrets(repoUrl.trim(), secretDraft);
      const secrets = await api.getSecrets(repoUrl.trim());
      setSavedSecretNames(secrets.names);
      setSecretDraft({});
      setStatus("Secrets saved locally");
    } catch (error) {
      setStatus(String(error));
    } finally {
      setBusy(false);
    }
  }

  async function startBuild() {
    setBusy(true);
    setLogs([]);
    setResult(null);
    setStatus("Build running...");
    setActiveTab("Logs");
    try {
      const build = await api.runBuild({
        repoUrl: repoUrl.trim(),
        refName: refName.trim(),
        outputFolder,
        workflowPath,
        jobId,
        shellMode: config.shellMode,
      });
      api.getToolStatus().then(setToolStatus).catch(() => undefined);
      setResult(build);
      setStatus(`Build complete: ${build.apkFiles.length} APK file(s) copied`);
    } catch (error) {
      setStatus(String(error));
    } finally {
      setBusy(false);
    }
  }

  async function cancelBuild() {
    await api.cancelBuild();
    setStatus("Cancellation requested");
  }

  async function installTools() {
    setInstallingTools(true);
    setLogs([]);
    setActiveTab("Logs");
    setStatus("Installing local build tools...");
    try {
      const status = await api.installBuildTools();
      setToolStatus(status);
      setStatus("Local build tools are ready");
    } catch (error) {
      setStatus(String(error));
    } finally {
      setInstallingTools(false);
    }
  }

  async function saveSettings(next: AppConfig) {
    setConfig(next);
    await api.saveConfig(next);
    setStatus("Settings saved");
  }

  return (
    <main className="app-root min-h-screen">
      {showSplash && (
        <div className={`app-splash ${splashExiting ? "leaving" : ""}`} aria-hidden="true">
          <img className="app-splash-logo" src="/apk-build-launcher-transparent.png" alt="" />
        </div>
      )}
      <div className="app-shell flex min-h-screen flex-col">
        <header className="app-toolbar">
          <div className="toolbar-controls" aria-hidden="true">
            <span>&lsaquo;</span>
            <span>&rsaquo;</span>
          </div>
          <div className="toolbar-menu" aria-hidden="true">
            <span>File</span>
            <span>Edit</span>
            <span>View</span>
            <span>Window</span>
            <span>Help</span>
          </div>
        </header>
        <div className="flex min-h-0 flex-1">
          <aside className="app-sidebar w-60 p-4">
          <h1 className="text-xl font-semibold tracking-normal">APK Build Launcher</h1>
          <p className="text-muted mt-2 text-sm">Workflow Adapter for Android APK builds</p>
          <nav className="mt-8 grid gap-1">
            {tabs.map((tab) => (
              <button
                key={tab}
                className={`tab-button ${activeTab === tab ? "active" : ""}`}
                onClick={() => setActiveTab(tab)}
              >
                {tab}
              </button>
            ))}
          </nav>
          <div className="status-card mt-8 rounded-md border p-3 text-xs">
            <div className="strong-text font-medium">Status</div>
            <div className="mt-1 break-words">{status}</div>
          </div>
          </aside>

          <section className="app-content-panel flex-1 overflow-auto p-6">
          {activeTab === "Home" && (
            <Panel title="Home">
              <div className="grid gap-4 lg:grid-cols-2">
                <Field label="GitHub repo URL">
                  <div className="input-action">
                    <input
                      className="input"
                      value={repoUrl}
                      onChange={(event) => setRepoUrl(event.target.value)}
                      onKeyDown={(event) => {
                        if (event.key === "Enter") {
                          void loadBranches();
                        }
                      }}
                      placeholder="https://github.com/org/repo.git"
                    />
                    <button className="button secondary action-button" disabled={busy || loadingBranches || !repoUrl.trim()} onClick={loadBranches} title="Load branches from this repo">
                      {loadingBranches ? "..." : "Check"}
                    </button>
                  </div>
                  {branchMessage && <span className="field-hint">{branchMessage}</span>}
                </Field>
                <Field label="Branch, PR branch, or PR number">
                  <input className="input" list="branch-options" value={refName} onChange={(event) => setRefName(event.target.value)} placeholder="dev or 123" />
                  <datalist id="branch-options">
                    {branches.map((branch) => <option key={branch} value={branch} />)}
                  </datalist>
                </Field>
                <Field label="Workflow">
                  <select className="input" value={workflowPath} onChange={(event) => setWorkflowPath(event.target.value)}>
                    <option value="">Detect workflows first</option>
                    {workflows.map((workflow) => <option key={workflow.filePath} value={workflow.filePath}>{workflow.name}</option>)}
                  </select>
                </Field>
                <Field label="Job">
                  <select className="input" value={jobId} onChange={(event) => setJobId(event.target.value)}>
                    <option value="">Select a workflow first</option>
                    {selectedWorkflow?.jobs.map((job) => <option key={job.id} value={job.id}>{job.name}</option>)}
                  </select>
                </Field>
                <Field label="Output folder">
                  <div className="flex gap-2">
                    <input className="input" value={outputFolder} onChange={(event) => setOutputFolder(event.target.value)} />
                    <button className="button secondary" onClick={async () => setOutputFolder(await api.chooseFolder())}>Browse</button>
                  </div>
                </Field>
                <Field label="Compatibility mode">
                  <select className="input" value={config.shellMode} onChange={(event) => saveSettings({ ...config, shellMode: event.target.value as ShellMode })}>
                    <option value="native">Native Windows</option>
                    <option value="bash">Git Bash</option>
                  </select>
                </Field>
              </div>
              <div className="mt-6 flex flex-wrap gap-3">
                <button className="button" disabled={busy || !repoUrl || !refName} onClick={prepareAndDetect}>Clone / Update & Detect</button>
                <button className="button" disabled={busy || !repoUrl || !outputFolder || !workflowPath || !jobId} onClick={startBuild}>Build APK</button>
                <button className="button secondary" disabled={busy || installingTools} onClick={installTools}>Install / Repair Tools</button>
                <button className="button danger" disabled={!busy} onClick={cancelBuild}>Cancel</button>
              </div>
              {selectedJob && <p className="text-muted mt-4 text-sm">Selected job runs on {selectedJob.runsOn} with {selectedJob.stepCount} steps. It will execute locally on Windows.</p>}
            </Panel>
          )}

          {activeTab === "Tools" && (
            <Panel title="Tools">
              <div className="grid gap-3 lg:grid-cols-2">
                <ToolRow label="Git" probe={toolStatus?.git} />
                <ToolRow label="Java 17" probe={toolStatus?.java} />
                <ToolRow label="Android SDK" probe={toolStatus?.androidSdk} />
                <ToolRow label="Git Bash" probe={toolStatus?.gitBash} />
              </div>
              {toolStatus?.toolsRoot && <p className="text-muted mt-4 break-words text-sm">Tools folder: {toolStatus.toolsRoot}</p>}
              <div className="mt-5 flex flex-wrap gap-3">
                <button className="button" disabled={busy || installingTools} onClick={installTools}>{installingTools ? "Installing..." : "Install / Repair Tools"}</button>
                <button className="button secondary" disabled={installingTools} onClick={() => api.getToolStatus().then(setToolStatus).catch((error) => setStatus(String(error)))}>Refresh</button>
              </div>
            </Panel>
          )}

          {activeTab === "Secrets" && (
            <Panel title="Secrets">
              <p className="text-muted mb-4 text-sm">Values are stored locally per repo and redacted from logs. Add the sample workflow secrets here if they are not already present.</p>
              <div className="grid gap-4 lg:grid-cols-2">
                {["LOCAL_PROPERTIES_BASE64", "LOCAL_DEV_PROPERTIES_BASE64"].map((name) => (
                  <Field key={name} label={`${name}${savedSecretNames.includes(name) ? " (saved)" : ""}`}>
                    <textarea className="input min-h-28" value={secretDraft[name] ?? ""} onChange={(event) => setSecretDraft({ ...secretDraft, [name]: event.target.value })} />
                  </Field>
                ))}
              </div>
              <button className="button mt-5" disabled={busy || !repoUrl} onClick={saveSecrets}>Save Secrets</button>
            </Panel>
          )}

          {activeTab === "Workflows" && (
            <Panel title="Workflows">
              <div className="space-y-3">
                {workflows.map((workflow) => (
                  <div key={workflow.filePath} className="surface-card rounded-md border p-4">
                    <div className="flex items-start justify-between gap-4">
                      <div>
                        <h2 className="font-medium">{workflow.name}</h2>
                        <p className="text-muted text-sm">{workflow.filePath}</p>
                        <p className="text-subtle mt-1 text-xs">Trigger: {workflow.trigger}</p>
                      </div>
                      <button className="button secondary" onClick={() => { setWorkflowPath(workflow.filePath); setJobId(workflow.jobs[0]?.id ?? ""); }}>Use</button>
                    </div>
                    <div className="mt-3 grid gap-2">
                      {workflow.jobs.map((job) => (
                        <div key={job.id} className="sub-surface rounded px-3 py-2 text-sm">
                          {job.name} | {job.runsOn} | {job.stepCount} steps
                        </div>
                      ))}
                    </div>
                  </div>
                ))}
                {!workflows.length && <p className="text-muted text-sm">No workflows detected yet.</p>}
              </div>
            </Panel>
          )}

          {activeTab === "Logs" && (
            <Panel title="Logs">
              <div className="log-box h-[65vh] overflow-auto rounded-md border p-4 font-mono text-xs leading-5">
                {logs.map((log, index) => <LogLine key={`${log.buildId}-${index}`} log={log} />)}
                {!logs.length && <span className="text-subtle">Build logs will appear here.</span>}
              </div>
              {result && <p className="success-text mt-4 text-sm">Copied {result.apkFiles.length} APK file(s) to {result.outputFolder}</p>}
            </Panel>
          )}

          {activeTab === "Settings" && (
            <Panel title="Settings">
              <div className="grid gap-4 lg:grid-cols-2">
                <Field label="Default repo folder">
                  <input className="input" value={config.defaultRepoFolder} onChange={(event) => setConfig({ ...config, defaultRepoFolder: event.target.value })} />
                </Field>
                <Field label="Default output folder">
                  <input className="input" value={config.defaultOutputFolder} onChange={(event) => setConfig({ ...config, defaultOutputFolder: event.target.value })} />
                </Field>
                <Field label="Shell mode">
                  <select className="input" value={config.shellMode} onChange={(event) => setConfig({ ...config, shellMode: event.target.value as ShellMode })}>
                    <option value="native">Native Windows</option>
                    <option value="bash">Git Bash</option>
                  </select>
                </Field>
                <Field label="Theme">
                  <select className="input" value={config.theme} onChange={(event) => setConfig({ ...config, theme: event.target.value as AppConfig["theme"] })}>
                    <option value="system">System</option>
                    <option value="dark">Dark</option>
                    <option value="light">Light</option>
                  </select>
                </Field>
              </div>
              <button className="button mt-5" onClick={() => saveSettings(config)}>Save Settings</button>
            </Panel>
          )}
          </section>
        </div>
      </div>
    </main>
  );
}

function Panel({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div>
      <h2 className="mb-5 text-2xl font-semibold">{title}</h2>
      {children}
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="field-label grid gap-2 text-sm">
      <span>{label}</span>
      {children}
    </label>
  );
}

function ToolRow({ label, probe }: { label: string; probe?: ToolProbe }) {
  const available = probe?.available ?? false;
  return (
    <div className="surface-card rounded-md border p-4">
      <div className="flex items-start justify-between gap-3">
        <div>
          <h2 className="font-medium">{label}</h2>
          <p className="text-muted mt-1 text-sm">{probe?.message ?? "Checking..."}</p>
          {probe?.path && <p className="text-subtle mt-2 break-words text-xs">{probe.path}</p>}
        </div>
        <span className={`tool-pill ${available ? "ready" : "missing"}`}>{available ? "Ready" : "Missing"}</span>
      </div>
    </div>
  );
}

function LogLine({ log }: { log: LogEvent }) {
  const color = {
    info: "text-zinc-300",
    success: "text-emerald-300",
    warn: "text-amber-300",
    error: "text-red-300",
    group: "text-cyan-300",
    endgroup: "text-zinc-500",
  }[log.level];
  return <div className={color}>{log.message}</div>;
}
