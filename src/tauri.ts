import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type { AppConfig, BranchSummary, BuildRequest, BuildResult, SecretSummary, ToolStatus, WorkflowSummary } from "./types";

export const api = {
  getConfig: () => invoke<AppConfig>("get_config"),
  saveConfig: (config: AppConfig) => invoke<void>("save_config", { config }),
  chooseFolder: async () => {
    const selected = await open({ directory: true, multiple: false });
    return typeof selected === "string" ? selected : "";
  },
  listBranches: (repoUrl: string) => invoke<BranchSummary[]>("list_branches", { repoUrl }),
  prepareRepo: (repoUrl: string, refName: string) =>
    invoke<string>("prepare_repo", { repoUrl, refName }),
  detectWorkflows: (repoPath: string) =>
    invoke<WorkflowSummary[]>("detect_workflows", { repoPath }),
  getSecrets: (repoUrl: string) => invoke<SecretSummary>("get_secrets", { repoUrl }),
  saveSecrets: (repoUrl: string, secrets: Record<string, string>) =>
    invoke<void>("save_secrets", { repoUrl, secrets }),
  getToolStatus: () => invoke<ToolStatus>("get_tool_status"),
  installBuildTools: () => invoke<ToolStatus>("install_build_tools"),
  runBuild: (request: BuildRequest) => invoke<BuildResult>("run_build", { request }),
  cancelBuild: () => invoke<void>("cancel_build"),
};
