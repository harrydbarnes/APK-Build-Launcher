import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "../tauri";
import type {
  AppConfig,
  BuildManifest,
  BuildPlan,
  BuildPreset,
  BuildRequest,
  BuildResult,
  BuildTarget,
  JobSummary,
  LogEvent,
  ShellMode,
  SourceMode,
  ToolStatus,
  WorkflowSummary,
} from "../types";

const LOG_LIMIT = 1200;

export type View = "home" | "workflows" | "logs" | "artifacts" | "settings";
export type BuildStateLabel = "idle" | "running" | "success" | "failed" | "cancelled";

export type BuildDraft = {
  sourceMode: SourceMode;
  repoUrl: string;
  localPath: string;
  refName: string;
  outputFolder: string;
  workflowPath: string;
  jobId: string;
  shellMode: ShellMode;
  target: BuildTarget;
};

export type LogLevelFilter = "all" | LogEvent["level"];

const emptyConfig: AppConfig = {
  defaultRepoFolder: "",
  defaultOutputFolder: "",
  shellMode: "native",
  theme: "system",
  presets: [],
  defaultPresetId: null,
};

const emptyDraft: BuildDraft = {
  sourceMode: "remote",
  repoUrl: "",
  localPath: "",
  refName: "dev",
  outputFolder: "",
  workflowPath: "",
  jobId: "",
  shellMode: "native",
  target: "auto",
};

function normalizeConfig(config: AppConfig): AppConfig {
  return {
    ...emptyConfig,
    ...config,
    presets: (config.presets ?? []).map((preset) => ({
      ...preset,
      sourceMode: preset.sourceMode ?? "remote",
      localPath: preset.localPath ?? "",
      target: preset.target ?? "auto",
    })),
    defaultPresetId: config.defaultPresetId ?? null,
  };
}

function draftFromPreset(preset: BuildPreset): BuildDraft {
  return {
    sourceMode: preset.sourceMode ?? "remote",
    repoUrl: preset.repoUrl,
    localPath: preset.localPath ?? "",
    refName: preset.refName,
    outputFolder: preset.outputFolder,
    workflowPath: preset.workflowPath,
    jobId: preset.jobId,
    shellMode: preset.shellMode,
    target: preset.target ?? "auto",
  };
}

function makePresetId() {
  return `preset-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

function nowStamp() {
  return new Date().toISOString();
}

function presetName(draft: BuildDraft) {
  const source = draft.sourceMode === "local" ? draft.localPath : draft.repoUrl;
  const repo = source.trim().replace(/[\\/]+$/, "").replace(/\.git$/, "").split(/[\\/:]/).filter(Boolean).pop();
  return `${repo || "Local build"} / ${draft.target === "auto" ? "auto" : draft.target}`;
}

function sourceName(draft: BuildDraft) {
  const source = draft.sourceMode === "local" ? draft.localPath : draft.repoUrl;
  return source.trim().replace(/[\\/]+$/, "").replace(/\.git$/, "").split(/[\\/:]/).filter(Boolean).pop() || "project";
}

function requestFromDraft(draft: BuildDraft): BuildRequest {
  return {
    sourceMode: draft.sourceMode,
    repoUrl: draft.repoUrl.trim(),
    localPath: draft.localPath.trim(),
    refName: draft.sourceMode === "local" ? (draft.refName.trim() || "local") : draft.refName.trim(),
    outputFolder: draft.outputFolder,
    workflowPath: draft.workflowPath,
    jobId: draft.jobId,
    shellMode: draft.shellMode,
    target: draft.target,
  };
}

function hasWorkflow(workflows: WorkflowSummary[], workflowPath: string) {
  return workflows.some((workflow) => workflow.filePath === workflowPath);
}

function hasJob(workflows: WorkflowSummary[], workflowPath: string, jobId: string) {
  return workflows
    .find((workflow) => workflow.filePath === workflowPath)
    ?.jobs.some((job) => job.id === jobId) ?? false;
}

export function useBuildLauncher() {
  const [activeView, setActiveView] = useState<View>("home");
  const [config, setConfig] = useState<AppConfig>(emptyConfig);
  const [draft, setDraft] = useState<BuildDraft>(emptyDraft);
  const [repoPath, setRepoPath] = useState("");
  const [workflows, setWorkflows] = useState<WorkflowSummary[]>([]);
  const [secretDraft, setSecretDraft] = useState<Record<string, string>>({});
  const [savedSecretNames, setSavedSecretNames] = useState<string[]>([]);
  const [logs, setLogs] = useState<LogEvent[]>([]);
  const [toolStatus, setToolStatus] = useState<ToolStatus | null>(null);
  const [branches, setBranches] = useState<string[]>([]);
  const [branchMessage, setBranchMessage] = useState("");
  const [status, setStatus] = useState("Ready");
  const [buildState, setBuildState] = useState<BuildStateLabel>("idle");
  const [result, setResult] = useState<BuildResult | null>(null);
  const [plan, setPlan] = useState<BuildPlan | null>(null);
  const [history, setHistory] = useState<BuildManifest[]>([]);
  const [analyzing, setAnalyzing] = useState(false);
  const [busy, setBusy] = useState(false);
  const [installingTools, setInstallingTools] = useState(false);
  const [loadingBranches, setLoadingBranches] = useState(false);
  const [appReady, setAppReady] = useState(false);
  const [logSearch, setLogSearch] = useState("");
  const [logLevel, setLogLevel] = useState<LogLevelFilter>("all");
  const [autoScrollLogs, setAutoScrollLogs] = useState(true);
  const [selectedPresetId, setSelectedPresetId] = useState<string | null>(null);
  const previousSource = useRef("");

  useEffect(() => {
    api.getConfig()
      .then((loaded) => {
        const normalized = normalizeConfig(loaded);
        const defaultPreset = normalized.presets.find((preset) => preset.id === normalized.defaultPresetId);
        setConfig(normalized);
        setSelectedPresetId(defaultPreset?.id ?? null);
        setDraft(defaultPreset ? draftFromPreset(defaultPreset) : {
          ...emptyDraft,
          outputFolder: normalized.defaultOutputFolder,
          shellMode: normalized.shellMode,
        });
      })
      .catch((error) => setStatus(String(error)))
      .finally(() => setAppReady(true));
    api.getToolStatus().then(setToolStatus).catch(() => undefined);
  }, []);

  useEffect(() => {
    document.documentElement.dataset.theme = config.theme;
  }, [config.theme]);

  useEffect(() => {
    if (!appReady) {
      previousSource.current = `${draft.sourceMode}:${draft.sourceMode === "local" ? draft.localPath : draft.repoUrl}`;
      return;
    }
    const source = `${draft.sourceMode}:${draft.sourceMode === "local" ? draft.localPath : draft.repoUrl}`;
    if (previousSource.current === source) {
      return;
    }
    previousSource.current = source;
    setBranches([]);
    setBranchMessage("");
    setRepoPath("");
    setWorkflows([]);
    setPlan(null);
    setHistory([]);
    setSavedSecretNames([]);
    setDraft((current) => ({ ...current, workflowPath: "", jobId: "" }));
  }, [appReady, draft.localPath, draft.repoUrl, draft.sourceMode]);

  useEffect(() => {
    setPlan(null);
  }, [draft.jobId, draft.shellMode, draft.target, draft.workflowPath]);

  useEffect(() => {
    const unlisten = listen<LogEvent>("build-log", (event) => {
      setLogs((items) => [...items, event.payload].slice(-LOG_LIMIT));
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const updateDraft = useCallback((patch: Partial<BuildDraft>) => {
    setDraft((current) => ({ ...current, ...patch }));
  }, []);

  const selectedWorkflow = useMemo(
    () => workflows.find((workflow) => workflow.filePath === draft.workflowPath),
    [workflows, draft.workflowPath],
  );

  const selectedJob = useMemo<JobSummary | undefined>(
    () => selectedWorkflow?.jobs.find((job) => job.id === draft.jobId),
    [selectedWorkflow, draft.jobId],
  );

  const filteredLogs = useMemo(() => {
    const query = logSearch.trim().toLocaleLowerCase();
    return logs.filter((log) => {
      const matchesLevel = logLevel === "all" || log.level === logLevel;
      const matchesQuery = !query || log.message.toLocaleLowerCase().includes(query);
      return matchesLevel && matchesQuery;
    });
  }, [logs, logLevel, logSearch]);

  const latestPath = useMemo(() => {
    if (!result || !draft.outputFolder.trim()) {
      return "";
    }
    return `${draft.outputFolder}\\${sourceName(draft)}\\latest`;
  }, [draft, result]);

  const readiness = useMemo(() => {
    const repoReady = draft.sourceMode === "local"
      ? Boolean(draft.localPath.trim())
      : Boolean(draft.repoUrl.trim() && draft.refName.trim());
    const workflowReady = !draft.workflowPath || hasWorkflow(workflows, draft.workflowPath);
    const jobReady = !draft.workflowPath || Boolean(draft.jobId && hasJob(workflows, draft.workflowPath, draft.jobId));
    const outputReady = Boolean(draft.outputFolder.trim());
    const gitReady = toolStatus?.git.available ?? false;
    const javaReady = toolStatus?.java.available ?? false;
    const androidReady = toolStatus?.androidSdk.available ?? false;
    const bashReady = draft.shellMode === "native" || (toolStatus?.gitBash.available ?? false);
    const secretsReady = plan ? plan.requiredSecrets.every((name) => savedSecretNames.includes(name)) : true;
    const planReady = plan?.supported ?? false;

    return {
      repoReady,
      workflowReady,
      jobReady,
      outputReady,
      gitReady,
      javaReady,
      androidReady,
      bashReady,
      secretsReady,
      planReady,
      canDetect: repoReady && !busy && !loadingBranches && !analyzing,
      canBuild: repoReady && outputReady && !busy && !analyzing,
    };
  }, [analyzing, busy, draft, loadingBranches, plan, savedSecretNames, toolStatus, workflows]);

  const saveConfig = useCallback(async (next: AppConfig, message = "Settings saved") => {
    const normalized = normalizeConfig(next);
    setConfig(normalized);
    await api.saveConfig(normalized);
    setStatus(message);
  }, []);

  const loadBranches = useCallback(async () => {
    const repoUrl = draft.repoUrl.trim();
    if (!repoUrl) {
      setBranchMessage("Enter a GitHub repo URL first.");
      return;
    }
    setLoadingBranches(true);
    setBranchMessage("Loading branches...");
    setLogs([]);
    try {
      const loaded = await api.listBranches(repoUrl);
      api.getToolStatus().then(setToolStatus).catch(() => undefined);
      setBranches(loaded);
      if (loaded.length && (!draft.refName.trim() || !loaded.includes(draft.refName.trim()))) {
        updateDraft({ refName: loaded[0] });
      }
      setBranchMessage(loaded.length ? `${loaded.length} branch${loaded.length === 1 ? "" : "es"} loaded.` : "No branches found.");
    } catch (error) {
      const message = String(error);
      setBranchMessage(message);
      setStatus(message);
    } finally {
      setLoadingBranches(false);
    }
  }, [draft.refName, draft.repoUrl, updateDraft]);

  const prepareAndDetect = useCallback(async () => {
    if (!readiness.repoReady) {
      setStatus("Add a repository URL and ref before detecting workflows.");
      return;
    }
    setBusy(true);
    setBuildState("idle");
    setStatus("Preparing tools and repository...");
    setResult(null);
    try {
      const path = draft.sourceMode === "local"
        ? draft.localPath.trim()
        : await api.prepareRepo(draft.repoUrl.trim(), draft.refName.trim());
      if (draft.sourceMode === "remote") {
        api.getToolStatus().then(setToolStatus).catch(() => undefined);
      }
      setRepoPath(path);
      setStatus("Detecting workflow files...");
      const detected = await api.detectWorkflows(path);
      setWorkflows(detected);
      const preferredWorkflow = detected.find((workflow) => workflow.filePath === draft.workflowPath) ?? detected[0];
      const preferredJob = preferredWorkflow?.jobs.find((job) => job.id === draft.jobId) ?? preferredWorkflow?.jobs[0];
      updateDraft({
        workflowPath: preferredWorkflow?.filePath ?? "",
        jobId: preferredJob?.id ?? "",
      });
      const secrets = await api.getSecrets(draft.sourceMode === "local" ? draft.localPath.trim() : draft.repoUrl.trim());
      setSavedSecretNames(secrets.names);
      setActiveView("workflows");
      setStatus(detected.length ? "Workflows detected; analyse a job or build directly" : "No workflow files found; native project detection will be used");
    } catch (error) {
      setBuildState("failed");
      setStatus(String(error));
    } finally {
      setBusy(false);
    }
  }, [draft, readiness.repoReady, updateDraft]);

  const saveSecrets = useCallback(async () => {
    const source = draft.sourceMode === "local" ? draft.localPath.trim() : draft.repoUrl.trim();
    if (!source) {
      setStatus("Choose a project source before saving secrets.");
      return;
    }
    setBusy(true);
    try {
      await api.saveSecrets(source, secretDraft);
      const secrets = await api.getSecrets(source);
      setSavedSecretNames(secrets.names);
      setSecretDraft({});
      setStatus("Secrets saved locally");
    } catch (error) {
      setStatus(String(error));
    } finally {
      setBusy(false);
    }
  }, [draft.localPath, draft.repoUrl, draft.sourceMode, secretDraft]);

  const analyzeCurrent = useCallback(async () => {
    if (!readiness.repoReady) {
      setStatus("Choose a Git repository or local project before analysing.");
      return null;
    }
    setAnalyzing(true);
    setStatus("Analysing the local build plan...");
    try {
      const request = requestFromDraft(draft);
      const analyzed = await api.analyzeBuild(request);
      setPlan(analyzed);
      const source = draft.sourceMode === "local" ? draft.localPath.trim() : draft.repoUrl.trim();
      const secrets = await api.getSecrets(source);
      setSavedSecretNames(secrets.names);
      setStatus(analyzed.supported ? `${analyzed.targetLabel} plan is ready` : analyzed.summary);
      return analyzed;
    } catch (error) {
      setStatus(String(error));
      setBuildState("failed");
      return null;
    } finally {
      setAnalyzing(false);
    }
  }, [draft, readiness.repoReady]);

  const startBuild = useCallback(async () => {
    if (!readiness.canBuild) {
      setStatus("Choose a project source and output folder before building.");
      return;
    }
    const request = requestFromDraft(draft);
    setBusy(true);
    setLogs([]);
    setResult(null);
    setBuildState("running");
    setStatus("Analysing build and preparing non-admin tools...");
    setActiveView("logs");
    try {
      const analyzed = await api.analyzeBuild(request);
      setPlan(analyzed);
      if (!analyzed.supported) {
        setActiveView("home");
        throw new Error(`Build plan has blockers: ${analyzed.blockers.join(" ")}`);
      }
      const source = draft.sourceMode === "local" ? draft.localPath.trim() : draft.repoUrl.trim();
      const secrets = await api.getSecrets(source);
      setSavedSecretNames(secrets.names);
      const missingSecrets = analyzed.requiredSecrets.filter((name) => !secrets.names.includes(name));
      if (missingSecrets.length) {
        setActiveView("home");
        throw new Error(`Save the required local secrets before building: ${missingSecrets.join(", ")}`);
      }
      setStatus("Installing or verifying required per-user tools...");
      setToolStatus(await api.installPlanTools(request));
      setStatus("Build running in an isolated workspace...");
      const build = await api.runBuild(request);
      api.getToolStatus().then(setToolStatus).catch(() => undefined);
      setResult(build);
      setBuildState("success");
      setActiveView("artifacts");
      setStatus(`Build complete: ${build.artifacts.length} artifact(s) published`);
      setHistory(await api.listBuildHistory(draft.outputFolder, sourceName(draft)));
    } catch (error) {
      const message = String(error);
      setBuildState(message.toLocaleLowerCase().includes("cancel") ? "cancelled" : "failed");
      setStatus(message);
    } finally {
      setBusy(false);
    }
  }, [draft, readiness.canBuild]);

  const cancelBuild = useCallback(async () => {
    await api.cancelBuild();
    setBuildState("cancelled");
    setStatus("Cancellation requested");
  }, []);

  const installTools = useCallback(async () => {
    setInstallingTools(true);
    setLogs([]);
    setActiveView("logs");
    setStatus("Installing local build tools...");
    try {
      const status = plan
        ? await api.installPlanTools(requestFromDraft(draft))
        : await api.installBuildTools();
      setToolStatus(status);
      setStatus("Local build tools are ready");
    } catch (error) {
      setStatus(String(error));
    } finally {
      setInstallingTools(false);
    }
  }, [draft, plan]);

  const refreshTools = useCallback(async () => {
    try {
      setToolStatus(await api.getToolStatus());
      setStatus("Tool status refreshed");
    } catch (error) {
      setStatus(String(error));
    }
  }, []);

  const chooseOutputFolder = useCallback(async () => {
    const selected = await api.chooseFolder();
    if (selected) {
      updateDraft({ outputFolder: selected });
    }
  }, [updateDraft]);

  const chooseLocalFolder = useCallback(async () => {
    const selected = await api.chooseFolder();
    if (selected) {
      updateDraft({ sourceMode: "local", localPath: selected, refName: "local" });
    }
  }, [updateDraft]);

  const openFolder = useCallback(async (path: string) => {
    try {
      await api.openPath(path);
    } catch (error) {
      setStatus(String(error));
    }
  }, []);

  useEffect(() => {
    const hasSource = draft.sourceMode === "local" ? draft.localPath.trim() : draft.repoUrl.trim();
    if (!appReady || !hasSource || !draft.outputFolder.trim()) {
      setHistory([]);
      return;
    }
    api.listBuildHistory(draft.outputFolder, sourceName(draft))
      .then(setHistory)
      .catch(() => setHistory([]));
  }, [appReady, draft.localPath, draft.outputFolder, draft.repoUrl, draft.sourceMode]);

  const chooseDefaultRepoFolder = useCallback(async () => {
    const selected = await api.chooseFolder();
    if (selected) {
      const next = { ...config, defaultRepoFolder: selected };
      await saveConfig(next, "Default repo folder saved");
    }
  }, [config, saveConfig]);

  const chooseDefaultOutputFolder = useCallback(async () => {
    const selected = await api.chooseFolder();
    if (selected) {
      const next = { ...config, defaultOutputFolder: selected };
      setDraft((current) => current.outputFolder ? current : { ...current, outputFolder: selected });
      await saveConfig(next, "Default output folder saved");
    }
  }, [config, saveConfig]);

  const saveCurrentAsPreset = useCallback(async () => {
    if (draft.sourceMode === "local" ? !draft.localPath.trim() : !draft.repoUrl.trim()) {
      setStatus("Choose a project source before saving a preset.");
      return;
    }
    const preset: BuildPreset = {
      id: makePresetId(),
      name: presetName(draft),
      repoUrl: draft.repoUrl.trim(),
      refName: draft.refName.trim() || "dev",
      workflowPath: draft.workflowPath,
      jobId: draft.jobId,
      outputFolder: draft.outputFolder,
      shellMode: draft.shellMode,
      updatedAt: nowStamp(),
      sourceMode: draft.sourceMode,
      localPath: draft.localPath.trim(),
      target: draft.target,
    };
    await saveConfig({
      ...config,
      presets: [preset, ...config.presets],
      defaultPresetId: config.defaultPresetId ?? preset.id,
    }, "Preset saved");
    setSelectedPresetId(preset.id);
  }, [config, draft, saveConfig]);

  const updateSelectedPreset = useCallback(async () => {
    if (!selectedPresetId) {
      await saveCurrentAsPreset();
      return;
    }
    const presets = config.presets.map((preset) => preset.id === selectedPresetId
      ? {
        ...preset,
        repoUrl: draft.repoUrl.trim(),
        refName: draft.refName.trim() || "dev",
        workflowPath: draft.workflowPath,
        jobId: draft.jobId,
        outputFolder: draft.outputFolder,
        shellMode: draft.shellMode,
        sourceMode: draft.sourceMode,
        localPath: draft.localPath.trim(),
        target: draft.target,
        updatedAt: nowStamp(),
      }
      : preset);
    await saveConfig({ ...config, presets }, "Preset updated");
  }, [config, draft, saveConfig, saveCurrentAsPreset, selectedPresetId]);

  const renamePreset = useCallback(async (id: string, name: string) => {
    const trimmed = name.trim();
    if (!trimmed) {
      return;
    }
    await saveConfig({
      ...config,
      presets: config.presets.map((preset) => preset.id === id ? { ...preset, name: trimmed, updatedAt: nowStamp() } : preset),
    }, "Preset renamed");
  }, [config, saveConfig]);

  const duplicatePreset = useCallback(async (preset: BuildPreset) => {
    const duplicate = {
      ...preset,
      id: makePresetId(),
      name: `${preset.name} copy`,
      updatedAt: nowStamp(),
    };
    await saveConfig({ ...config, presets: [duplicate, ...config.presets] }, "Preset duplicated");
  }, [config, saveConfig]);

  const deletePreset = useCallback(async (id: string) => {
    const presets = config.presets.filter((preset) => preset.id !== id);
    const nextDefault = config.defaultPresetId === id ? presets[0]?.id ?? null : config.defaultPresetId ?? null;
    await saveConfig({ ...config, presets, defaultPresetId: nextDefault }, "Preset deleted");
    if (selectedPresetId === id) {
      setSelectedPresetId(nextDefault);
      const nextPreset = presets.find((preset) => preset.id === nextDefault);
      if (nextPreset) {
        setDraft(draftFromPreset(nextPreset));
      }
    }
  }, [config, saveConfig, selectedPresetId]);

  const setDefaultPreset = useCallback(async (id: string) => {
    await saveConfig({ ...config, defaultPresetId: id }, "Default preset updated");
  }, [config, saveConfig]);

  const selectPreset = useCallback((preset: BuildPreset) => {
    setSelectedPresetId(preset.id);
    setDraft(draftFromPreset(preset));
    setWorkflows([]);
    setPlan(null);
    setHistory([]);
    setRepoPath("");
    setBranchMessage("");
    setStatus(`Loaded preset: ${preset.name}`);
  }, []);

  const saveSettings = useCallback(async (next: AppConfig) => {
    await saveConfig(normalizeConfig(next));
    setDraft((current) => ({
      ...current,
      outputFolder: current.outputFolder || next.defaultOutputFolder,
      shellMode: next.shellMode,
    }));
  }, [saveConfig]);

  return {
    activeView,
    analyzing,
    appReady,
    autoScrollLogs,
    branches,
    branchMessage,
    buildState,
    busy,
    config,
    draft,
    filteredLogs,
    installingTools,
    history,
    latestPath,
    loadingBranches,
    logLevel,
    logs,
    logSearch,
    readiness,
    repoPath,
    result,
    plan,
    savedSecretNames,
    secretDraft,
    selectedJob,
    selectedPresetId,
    selectedWorkflow,
    status,
    toolStatus,
    workflows,
    cancelBuild,
    analyzeCurrent,
    chooseDefaultOutputFolder,
    chooseDefaultRepoFolder,
    chooseOutputFolder,
    chooseLocalFolder,
    deletePreset,
    duplicatePreset,
    installTools,
    loadBranches,
    openFolder,
    prepareAndDetect,
    refreshTools,
    renamePreset,
    saveCurrentAsPreset,
    saveSecrets,
    saveSettings,
    selectPreset,
    setActiveView,
    setAutoScrollLogs,
    setDefaultPreset,
    setLogLevel,
    setLogSearch,
    setSecretDraft,
    startBuild,
    updateDraft,
    updateSelectedPreset,
  };
}
