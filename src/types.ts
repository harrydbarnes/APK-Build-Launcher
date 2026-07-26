export type ShellMode = "native" | "bash";
export type SourceMode = "remote" | "local";
export type BuildTarget = "auto" | "android" | "tauri" | "electron" | "dotnet";

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
  sourceMode: SourceMode;
  localPath: string;
  target: BuildTarget;
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
  node: ToolProbe;
  rust: ToolProbe;
  dotnet: ToolProbe;
};

export type BuildRequest = {
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

export type LogEvent = {
  buildId: string;
  level: "info" | "success" | "warn" | "error" | "group" | "endgroup";
  message: string;
};

export type BuildResult = {
  buildId: string;
  outputFolder: string;
  target: BuildTarget;
  artifacts: PublishedArtifact[];
  manifestPath: string;
};

export type StepDisposition = "run" | "setup" | "collect" | "skip" | "block";

export type PlannedStep = {
  name: string;
  disposition: StepDisposition;
  command?: string | null;
  shell?: string | null;
  workingDirectory?: string | null;
  reason: string;
  continueOnError: boolean;
};

export type ToolRequirement = {
  id: string;
  label: string;
  version?: string | null;
  portable: boolean;
  reason: string;
};

export type ArtifactRule = {
  pattern: string;
  kind: string;
  required: boolean;
};

export type BuildPlan = {
  target: BuildTarget;
  targetLabel: string;
  supported: boolean;
  summary: string;
  steps: PlannedStep[];
  tools: ToolRequirement[];
  requiredSecrets: string[];
  artifacts: ArtifactRule[];
  blockers: string[];
  warnings: string[];
};

export type PublishedArtifact = {
  kind: string;
  path: string;
  relativePath: string;
  sizeBytes: number;
  sha256: string;
};

export type BuildManifest = {
  buildId: string;
  source: string;
  revision: string;
  target: string;
  createdAt: string;
  outputFolder: string;
  logFile?: string | null;
  artifacts: PublishedArtifact[];
};
