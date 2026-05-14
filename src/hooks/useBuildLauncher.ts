import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "../tauri";
import type {
  AppConfig,
  BuildPreset,
  BuildRequest,
  BuildResult,
  JobSummary,
  LogEvent,
  ShellMode,
  ToolStatus,
  WorkflowSummary,
} from "../types";

const LOG_LIMIT = 1200;

export type View = "home" | "workflows" | "logs" | "artifacts" | "settings";
export type BuildStateLabel = "idle" | "running" | "success" | "failed" | "cancelled";

export type BuildDraft = {
  repoUrl: string;
  refName: string;
  outputFolder: string;
  workflowPath: string;
  jobId: string;
  shellMode: ShellMode;
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
  repoUrl: "",
  refName: "dev",
  outputFolder: "",
  workflowPath: "",
  jobId: "",
  shellMode: "native",
};

type ParsedRepoInput = {
  repoUrl: string;
  branchHint: string;
};

function parseGitHubRepoInput(input: string): ParsedRepoInput {
  const trimmed = input.trim();
  if (!trimmed) {
    return { repoUrl: "", branchHint: "" };
  }

  let url: URL;
  try {
    url = new URL(trimmed);
  } catch {
    return { repoUrl: trimmed, branchHint: "" };
  }

  if (!["github.com", "www.github.com"].includes(url.hostname.toLocaleLowerCase())) {
    return { repoUrl: trimmed, branchHint: "" };
  }

  const segments = url.pathname.split("/").filter(Boolean);
  if (segments.length < 2) {
    return { repoUrl: trimmed.replace(/\/+$/, ""), branchHint: "" };
  }

  const owner = segments[0];
  const repoSegment = segments[1];
  const repo = repoSegment.replace(/\.git$/, "");
  const repoUrl = `${url.protocol}//${url.host}/${owner}/${repo}${repoSegment.endsWith(".git") ? ".git" : ""}`;
  const branchHint = segments[2] === "tree" || segments[2] === "blob"
    ? decodeRepoSegment(segments.slice(3).join("/"))
    : "";

  return { repoUrl, branchHint };
}

function decodeRepoSegment(segment: string) {
  try {
    return decodeURIComponent(segment.trim());
  } catch {
    return segment.trim();
  }
}

function resolveBranchHint(branches: string[], branchHint: string) {
  const normalizedHint = branchHint.trim().replace(/^refs\/heads\//, "").replace(/^origin\//, "");
  if (!normalizedHint) {
    return "";
  }
  if (branches.includes(normalizedHint)) {
    return normalizedHint;
  }
  return branches
    .filter((branch) => normalizedHint.startsWith(`${branch}/`))
    .sort((left, right) => right.length - left.length)[0] ?? normalizedHint;
}

function normalizeDraftRepoInput(draft: BuildDraft): BuildDraft {
  const parsed = parseGitHubRepoInput(draft.repoUrl);
  return {
    ...draft,
    repoUrl: parsed.repoUrl,
    refName: parsed.branchHint || draft.refName,
  };
}

function normalizeConfig(config: AppConfig): AppConfig {
  return {
    ...emptyConfig,
    ...config,
    presets: config.presets ?? [],
    defaultPresetId: config.defaultPresetId ?? null,
  };
}

function draftFromPreset(preset: BuildPreset): BuildDraft {
  return {
    repoUrl: preset.repoUrl,
    refName: preset.refName,
    outputFolder: preset.outputFolder,
    workflowPath: preset.workflowPath,
    jobId: preset.jobId,
    shellMode: preset.shellMode,
  };
}

function makePresetId() {
  return `preset-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

function nowStamp() {
  return new Date().toISOString();
}

function presetName(draft: BuildDraft) {
  const repo = draft.repoUrl.trim().replace(/\.git$/, "").split(/[/:]/).filter(Boolean).pop();
  return `${repo || "APK build"} / ${draft.refName.trim() || "ref"}`;
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
  const [busy, setBusy] = useState(false);
  const [installingTools, setInstallingTools] = useState(false);
  const [loadingBranches, setLoadingBranches] = useState(false);
  const [appReady, setAppReady] = useState(false);
  const [logSearch, setLogSearch] = useState("");
  const [logLevel, setLogLevel] = useState<LogLevelFilter>("all");
  const [autoScrollLogs, setAutoScrollLogs] = useState(true);
  const [selectedPresetId, setSelectedPresetId] = useState<string | null>(null);
  const previousRepoUrl = useRef(emptyDraft.repoUrl);

  useEffect(() => {
    api.getConfig()
      .then((loaded) => {
        const normalized = normalizeConfig(loaded);
        const defaultPreset = normalized.presets.find((preset) => preset.id === normalized.defaultPresetId);
        setConfig(normalized);
        setSelectedPresetId(defaultPreset?.id ?? null);
        const initialDraft = normalizeDraftRepoInput(defaultPreset ? draftFromPreset(defaultPreset) : {
          ...emptyDraft,
          outputFolder: normalized.defaultOutputFolder,
          shellMode: normalized.shellMode,
        });
        previousRepoUrl.current = initialDraft.repoUrl;
        setDraft(initialDraft);
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
      previousRepoUrl.current = draft.repoUrl;
      return;
    }
    if (previousRepoUrl.current === draft.repoUrl) {
      return;
    }
    previousRepoUrl.current = draft.repoUrl;
    setBranches([]);
    setBranchMessage("");
    setRepoPath("");
    setWorkflows([]);
    setSavedSecretNames([]);
    setDraft((current) => ({ ...current, workflowPath: "", jobId: "" }));
  }, [appReady, draft.repoUrl]);

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

  const setRepoUrl = useCallback((repoInput: string) => {
    const parsed = parseGitHubRepoInput(repoInput);
    updateDraft({
      repoUrl: parsed.repoUrl,
      ...(parsed.branchHint ? { refName: parsed.branchHint } : {}),
    });
  }, [updateDraft]);

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
    if (!result || !draft.repoUrl.trim() || !draft.outputFolder.trim()) {
      return "";
    }
    const repoName = draft.repoUrl.trim().replace(/\.git$/, "").split(/[/:]/).filter(Boolean).pop() || "repo";
    return `${draft.outputFolder}\\${repoName}\\latest`;
  }, [draft.outputFolder, draft.repoUrl, result]);

  const readiness = useMemo(() => {
    const repoReady = Boolean(draft.repoUrl.trim() && draft.refName.trim());
    const workflowReady = Boolean(draft.workflowPath && hasWorkflow(workflows, draft.workflowPath));
    const jobReady = Boolean(draft.jobId && hasJob(workflows, draft.workflowPath, draft.jobId));
    const outputReady = Boolean(draft.outputFolder.trim());
    const gitReady = toolStatus?.git.available ?? false;
    const javaReady = toolStatus?.java.available ?? false;
    const androidReady = toolStatus?.androidSdk.available ?? false;
    const bashReady = draft.shellMode === "native" || (toolStatus?.gitBash.available ?? false);
    const secretsReady = savedSecretNames.includes("LOCAL_PROPERTIES_BASE64")
      && savedSecretNames.includes("LOCAL_DEV_PROPERTIES_BASE64");

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
      canDetect: repoReady && !busy && !installingTools && !loadingBranches,
      canBuild: repoReady && workflowReady && jobReady && outputReady && !busy && !installingTools,
      canCancel: buildState === "running",
    };
  }, [buildState, busy, draft, installingTools, loadingBranches, savedSecretNames, toolStatus, workflows]);

  const saveConfig = useCallback(async (next: AppConfig, message = "Settings saved") => {
    const normalized = normalizeConfig(next);
    try {
      await api.saveConfig(normalized);
      setConfig(normalized);
      setStatus(message);
      return true;
    } catch (error) {
      setStatus(error instanceof Error ? error.message : "Failed to save configuration");
      return false;
    }
  }, []);

  const loadBranches = useCallback(async () => {
    const { repoUrl, branchHint } = parseGitHubRepoInput(draft.repoUrl);
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
      const nextRefName = branchHint
        ? resolveBranchHint(loaded, branchHint)
        : loaded.length && (!draft.refName.trim() || !loaded.includes(draft.refName.trim()))
          ? loaded[0]
          : draft.refName.trim();
      if (repoUrl !== draft.repoUrl || nextRefName !== draft.refName) {
        updateDraft({
          ...(repoUrl !== draft.repoUrl ? { repoUrl } : {}),
          ...(nextRefName !== draft.refName ? { refName: nextRefName } : {}),
        });
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
      const path = await api.prepareRepo(draft.repoUrl.trim(), draft.refName.trim());
      api.getToolStatus().then(setToolStatus).catch(() => undefined);
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
      const secrets = await api.getSecrets(draft.repoUrl.trim());
      setSavedSecretNames(secrets.names);
      setActiveView("workflows");
      setStatus(detected.length ? "Workflows detected" : "No workflow files found");
    } catch (error) {
      setBuildState("failed");
      setStatus(String(error));
    } finally {
      setBusy(false);
    }
  }, [draft.jobId, draft.refName, draft.repoUrl, draft.workflowPath, readiness.repoReady, updateDraft]);

  const saveSecrets = useCallback(async () => {
    if (!draft.repoUrl.trim()) {
      setStatus("Add a repository URL before saving secrets.");
      return;
    }
    setBusy(true);
    try {
      await api.saveSecrets(draft.repoUrl.trim(), secretDraft);
      const secrets = await api.getSecrets(draft.repoUrl.trim());
      setSavedSecretNames(secrets.names);
      setSecretDraft({});
      setStatus("Secrets saved locally");
    } catch (error) {
      setStatus(String(error));
    } finally {
      setBusy(false);
    }
  }, [draft.repoUrl, secretDraft]);

  const startBuild = useCallback(async () => {
    if (!readiness.canBuild) {
      setStatus("Complete repository, workflow, job, and output folder before building.");
      return;
    }
    const request: BuildRequest = {
      repoUrl: draft.repoUrl.trim(),
      refName: draft.refName.trim(),
      outputFolder: draft.outputFolder,
      workflowPath: draft.workflowPath,
      jobId: draft.jobId,
      shellMode: draft.shellMode,
    };
    setBusy(true);
    setLogs([]);
    setResult(null);
    setBuildState("running");
    setStatus("Build running...");
    setActiveView("logs");
    try {
      const build = await api.runBuild(request);
      api.getToolStatus().then(setToolStatus).catch(() => undefined);
      setResult(build);
      setBuildState("success");
      setActiveView("artifacts");
      setStatus(`Build complete: ${build.apkFiles.length} APK file(s) copied`);
    } catch (error) {
      const message = String(error);
      setBuildState(message.toLocaleLowerCase().includes("cancel") ? "cancelled" : "failed");
      setStatus(message);
    } finally {
      setBusy(false);
    }
  }, [draft, readiness.canBuild]);

  const cancelBuild = useCallback(async () => {
    try {
      await api.cancelBuild();
      setBuildState("cancelled");
      setStatus("Cancellation requested");
    } catch (error) {
      setStatus(error instanceof Error ? error.message : "Failed to cancel build");
    }
  }, []);

  const installTools = useCallback(async () => {
    setInstallingTools(true);
    setLogs([]);
    setActiveView("logs");
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
  }, []);

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
    if (!draft.repoUrl.trim()) {
      setStatus("Add a repository URL before saving a preset.");
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
    const newDraft = normalizeDraftRepoInput(draftFromPreset(preset));
    previousRepoUrl.current = newDraft.repoUrl;
    setDraft(newDraft);
    setWorkflows([]);
    setRepoPath("");
    setBranchMessage("");
    setBranches([]);
    setSavedSecretNames([]);
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
    latestPath,
    loadingBranches,
    logLevel,
    logs,
    logSearch,
    readiness,
    repoPath,
    result,
    savedSecretNames,
    secretDraft,
    selectedJob,
    selectedPresetId,
    selectedWorkflow,
    status,
    toolStatus,
    workflows,
    cancelBuild,
    chooseDefaultOutputFolder,
    chooseDefaultRepoFolder,
    chooseOutputFolder,
    deletePreset,
    duplicatePreset,
    installTools,
    loadBranches,
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
    setRepoUrl,
    updateDraft,
    updateSelectedPreset,
  };
}

export type BuildLauncher = ReturnType<typeof useBuildLauncher>;
