export type ShellMode = "native" | "bash";

export type AppConfig = {
  defaultRepoFolder: string;
  defaultOutputFolder: string;
  shellMode: ShellMode;
  theme: "system" | "light" | "dark";
  presets: BuildPreset[];
  defaultPresetId?: string | null;
};

export type BuildPreset = {
  id: string;
  name: string;
  repoUrl: string;
  refName: string;
  workflowPath: string;
  jobId: string;
  outputFolder: string;
  shellMode: ShellMode;
  updatedAt: string;
};

export type WorkflowSummary = {
  id: string;
  filePath: string;
  name: string;
  trigger: string;
  jobs: JobSummary[];
};

export type BranchSummary = string;

export type JobSummary = {
  id: string;
  name: string;
  runsOn: string;
  stepCount: number;
};

export type SecretSummary = {
  repoKey: string;
  names: string[];
};

export type ToolProbe = {
  available: boolean;
  path?: string | null;
  message: string;
};

export type ToolStatus = {
  toolsRoot: string;
  git: ToolProbe;
  java: ToolProbe;
  androidSdk: ToolProbe;
  gitBash: ToolProbe;
};

export type BuildRequest = {
  repoUrl: string;
  refName: string;
  outputFolder: string;
  workflowPath: string;
  jobId: string;
  shellMode: ShellMode;
};

export type LogEvent = {
  buildId: string;
  level: "info" | "success" | "warn" | "error" | "group" | "endgroup";
  message: string;
};

export type BuildResult = {
  buildId: string;
  outputFolder: string;
  apkFiles: string[];
};
