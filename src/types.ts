export type ShellMode = "native" | "bash";

export type AppConfig = {
  defaultRepoFolder: string;
  defaultOutputFolder: string;
  shellMode: ShellMode;
  theme: "system" | "light" | "dark";
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
