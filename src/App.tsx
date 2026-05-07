import { listen } from "@tauri-apps/api/event";
import { useEffect, useMemo, useState } from "react";
import { api } from "./tauri";
import type { AppConfig, BuildResult, JobSummary, LogEvent, ShellMode, WorkflowSummary } from "./types";

const emptyConfig: AppConfig = {
  defaultRepoFolder: "",
  defaultOutputFolder: "",
  shellMode: "native",
  theme: "system",
};

const tabs = ["Home", "Secrets", "Workflows", "Logs", "Settings"] as const;
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

  useEffect(() => {
    api.getConfig().then((loaded) => {
      setConfig(loaded);
      setOutputFolder(loaded.defaultOutputFolder);
    }).catch((error) => setStatus(String(error)));
  }, []);

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

  async function prepareAndDetect() {
    setBusy(true);
    setStatus("Cloning or updating repository...");
    setResult(null);
    try {
      const path = await api.prepareRepo(repoUrl.trim(), refName.trim());
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

  async function saveSettings(next: AppConfig) {
    setConfig(next);
    await api.saveConfig(next);
    setStatus("Settings saved");
  }

  return (
    <main className="min-h-screen bg-zinc-950 text-zinc-100">
      <div className="flex min-h-screen">
        <aside className="w-60 border-r border-zinc-800 bg-zinc-900/80 p-4">
          <h1 className="text-xl font-semibold tracking-normal">APK Build Launcher</h1>
          <p className="mt-2 text-sm text-zinc-400">Workflow Adapter for Android APK builds</p>
          <nav className="mt-8 grid gap-1">
            {tabs.map((tab) => (
              <button
                key={tab}
                className={`rounded-md px-3 py-2 text-left text-sm transition ${
                  activeTab === tab ? "bg-cyan-500 text-zinc-950" : "text-zinc-300 hover:bg-zinc-800"
                }`}
                onClick={() => setActiveTab(tab)}
              >
                {tab}
              </button>
            ))}
          </nav>
          <div className="mt-8 rounded-md border border-zinc-800 p-3 text-xs text-zinc-400">
            <div className="font-medium text-zinc-200">Status</div>
            <div className="mt-1 break-words">{status}</div>
          </div>
        </aside>

        <section className="flex-1 overflow-auto p-6">
          {activeTab === "Home" && (
            <Panel title="Home">
              <div className="grid gap-4 lg:grid-cols-2">
                <Field label="GitHub repo URL">
                  <input className="input" value={repoUrl} onChange={(event) => setRepoUrl(event.target.value)} placeholder="https://github.com/org/repo.git" />
                </Field>
                <Field label="Branch, PR branch, or PR number">
                  <input className="input" value={refName} onChange={(event) => setRefName(event.target.value)} placeholder="dev or 123" />
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
                <button className="button danger" disabled={!busy} onClick={cancelBuild}>Cancel</button>
              </div>
              {selectedJob && <p className="mt-4 text-sm text-zinc-400">Selected job runs on {selectedJob.runsOn} with {selectedJob.stepCount} steps. It will execute locally on Windows.</p>}
            </Panel>
          )}

          {activeTab === "Secrets" && (
            <Panel title="Secrets">
              <p className="mb-4 text-sm text-zinc-400">Values are stored locally per repo and redacted from logs. Add the sample workflow secrets here if they are not already present.</p>
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
                  <div key={workflow.filePath} className="rounded-md border border-zinc-800 p-4">
                    <div className="flex items-start justify-between gap-4">
                      <div>
                        <h2 className="font-medium">{workflow.name}</h2>
                        <p className="text-sm text-zinc-400">{workflow.filePath}</p>
                        <p className="mt-1 text-xs text-zinc-500">Trigger: {workflow.trigger}</p>
                      </div>
                      <button className="button secondary" onClick={() => { setWorkflowPath(workflow.filePath); setJobId(workflow.jobs[0]?.id ?? ""); }}>Use</button>
                    </div>
                    <div className="mt-3 grid gap-2">
                      {workflow.jobs.map((job) => (
                        <div key={job.id} className="rounded bg-zinc-900 px-3 py-2 text-sm">
                          {job.name} · {job.runsOn} · {job.stepCount} steps
                        </div>
                      ))}
                    </div>
                  </div>
                ))}
                {!workflows.length && <p className="text-sm text-zinc-400">No workflows detected yet.</p>}
              </div>
            </Panel>
          )}

          {activeTab === "Logs" && (
            <Panel title="Logs">
              <div className="h-[65vh] overflow-auto rounded-md border border-zinc-800 bg-black p-4 font-mono text-xs leading-5">
                {logs.map((log, index) => <LogLine key={`${log.buildId}-${index}`} log={log} />)}
                {!logs.length && <span className="text-zinc-500">Build logs will appear here.</span>}
              </div>
              {result && <p className="mt-4 text-sm text-emerald-300">Copied {result.apkFiles.length} APK file(s) to {result.outputFolder}</p>}
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
    <label className="grid gap-2 text-sm text-zinc-300">
      <span>{label}</span>
      {children}
    </label>
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
