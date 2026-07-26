use base64::Engine;
use chrono::Local;
use glob::glob;
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
};
use tauri::{AppHandle, Emitter, State};
use artifacts::{BuildManifest, PublishedArtifact};
use planning::{BuildPlan, BuildTarget, JobDoc, StepDisposition, StepDoc};
use workspace::{IsolatedWorkspace, SourceMode};

mod artifacts;
mod planning;
mod tools;
mod workspace;

#[derive(Default)]
struct BuildState {
    cancel: Arc<AtomicBool>,
    running: Mutex<Option<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppConfig {
    default_repo_folder: String,
    default_output_folder: String,
    shell_mode: ShellMode,
    theme: String,
    #[serde(default)]
    presets: Vec<BuildPreset>,
    #[serde(default)]
    default_preset_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BuildPreset {
    id: String,
    name: String,
    repo_url: String,
    ref_name: String,
    workflow_path: String,
    job_id: String,
    output_folder: String,
    shell_mode: ShellMode,
    updated_at: String,
    #[serde(default)]
    source_mode: SourceMode,
    #[serde(default)]
    local_path: String,
    #[serde(default)]
    target: BuildTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum ShellMode {
    Native,
    Bash,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            default_repo_folder: default_repo_root().display().to_string(),
            default_output_folder: dirs::download_dir().unwrap_or_else(default_app_data).display().to_string(),
            shell_mode: ShellMode::Native,
            theme: "system".to_string(),
            presets: vec![],
            default_preset_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkflowSummary {
    id: String,
    file_path: String,
    name: String,
    trigger: String,
    jobs: Vec<JobSummary>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct JobSummary {
    id: String,
    name: String,
    runs_on: String,
    step_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SecretSummary {
    repo_key: String,
    names: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolStatusResponse {
    tools_root: String,
    git: tools::ToolProbe,
    java: tools::ToolProbe,
    android_sdk: tools::ToolProbe,
    git_bash: tools::ToolProbe,
    node: tools::ToolProbe,
    rust: tools::ToolProbe,
    dotnet: tools::ToolProbe,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BuildRequest {
    #[serde(default)]
    source_mode: SourceMode,
    repo_url: String,
    #[serde(default)]
    local_path: String,
    ref_name: String,
    output_folder: String,
    workflow_path: String,
    job_id: String,
    shell_mode: ShellMode,
    #[serde(default)]
    target: BuildTarget,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BuildResult {
    build_id: String,
    output_folder: String,
    target: BuildTarget,
    artifacts: Vec<PublishedArtifact>,
    manifest_path: String,
}

#[derive(Debug, Clone, Serialize)]
struct LogEvent {
    build_id: String,
    level: String,
    message: String,
}

#[derive(Debug, Clone, Deserialize)]
struct WorkflowDoc {
    name: Option<String>,
    #[serde(default)]
    on: Value,
    #[serde(default)]
    env: BTreeMap<String, Value>,
    jobs: BTreeMap<String, DetectionJobDoc>,
}

#[derive(Debug, Clone, Deserialize)]
struct DetectionJobDoc {
    name: Option<String>,
    #[serde(rename = "runs-on", default)]
    runs_on: Value,
    #[serde(default)]
    steps: Vec<Value>,
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(BuildState::default())
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            list_branches,
            prepare_repo,
            detect_workflows,
            get_secrets,
            save_secrets,
            get_tool_status,
            install_build_tools,
            install_plan_tools,
            analyze_build,
            list_build_history,
            open_path,
            run_build,
            cancel_build
        ])
        .run(tauri::generate_context!())
        .expect("error while running APK Build Launcher");
}

#[tauri::command]
fn get_config() -> Result<AppConfig, String> {
    let path = config_path();
    if !path.exists() {
        return Ok(AppConfig::default());
    }
    let text = fs::read_to_string(&path).map_err(display_err)?;
    serde_json::from_str(&text).map_err(display_err)
}

#[tauri::command]
fn save_config(config: AppConfig) -> Result<(), String> {
    let path = config_path();
    ensure_parent(&path)?;
    let text = serde_json::to_string_pretty(&config).map_err(display_err)?;
    fs::write(path, text).map_err(display_err)
}

#[tauri::command]
fn list_branches(app: AppHandle, repo_url: String) -> Result<Vec<String>, String> {
    tools::ensure_git(|level, message| log(&app, "tools", level, message))?;
    let output = run_output(git_spec(["ls-remote", "--symref", repo_url.as_str(), "HEAD", "refs/heads/*"]), None)?;
    Ok(parse_remote_branches(&output))
}

#[tauri::command]
fn prepare_repo(app: AppHandle, repo_url: String, ref_name: String) -> Result<String, String> {
    tools::ensure_git(|level, message| log(&app, "tools", level, message))?;
    prepare_repo_inner(repo_url, ref_name, true)
}

fn prepare_repo_inner(repo_url: String, ref_name: String, update_remote: bool) -> Result<String, String> {
    let repo_path = repo_path_for(&repo_url)?;
    if repo_path.exists() {
        if update_remote {
            run_checked(
                git_spec(["fetch", "--all", "--prune"]).cwd(&repo_path),
                None,
            )?;
        } else if repo_matches_ref(&repo_path, &ref_name) {
            protect_local_properties(&repo_path)?;
            return Ok(repo_path.display().to_string());
        }
    } else {
        ensure_dir(&configured_repo_root())?;
        let repo_path_text = repo_path.to_string_lossy().to_string();
        run_checked(git_spec(["clone", repo_url.as_str(), repo_path_text.as_str()]), None)?;
    }

    checkout_ref(&repo_path, &ref_name)?;
    // This checkout lives under the launcher's managed repository cache. Keeping it exact
    // prevents old generated files or previous adapter edits from entering a new snapshot.
    run_checked(git_spec(["reset", "--hard", "HEAD"]).cwd(&repo_path), None)?;
    run_checked(git_spec(["clean", "-fdx"]).cwd(&repo_path), None)?;
    protect_local_properties(&repo_path)?;
    Ok(repo_path.display().to_string())
}

#[tauri::command]
fn detect_workflows(repo_path: String) -> Result<Vec<WorkflowSummary>, String> {
    let repo_root = PathBuf::from(repo_path);
    let workflow_dir = repo_root.join(".github").join("workflows");
    if !workflow_dir.exists() {
        return Ok(vec![]);
    }

    let mut workflows = vec![];
    for entry in fs::read_dir(workflow_dir).map_err(display_err)? {
        let entry = entry.map_err(display_err)?;
        let path = entry.path();
        let ext = path.extension().and_then(|x| x.to_str()).unwrap_or_default();
        if !matches!(ext, "yml" | "yaml") {
            continue;
        }
        let text = fs::read_to_string(&path).map_err(display_err)?;
        let doc: WorkflowDoc = serde_yaml::from_str(&text).map_err(|err| format!("{}: {}", path.display(), err))?;
        let name = doc.name.clone().unwrap_or_else(|| path.file_stem().unwrap_or_default().to_string_lossy().to_string());
        workflows.push(WorkflowSummary {
            id: stable_id(path.to_string_lossy().as_ref()),
            file_path: path
                .strip_prefix(&repo_root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/"),
            name,
            trigger: summarize_trigger(&doc.on),
            jobs: doc.jobs.iter().map(|(id, job)| JobSummary {
                id: id.clone(),
                name: job.name.clone().unwrap_or_else(|| id.clone()),
                runs_on: runs_on_to_string(&job.runs_on),
                step_count: job.steps.len(),
            }).collect(),
        });
    }
    Ok(workflows)
}

#[tauri::command]
fn get_secrets(repo_url: String) -> Result<SecretSummary, String> {
    let repo_key = stable_id(&repo_url);
    let secrets = read_secret_store(&repo_key)?;
    Ok(SecretSummary {
        repo_key,
        names: secrets.keys().cloned().collect(),
    })
}

#[tauri::command]
fn save_secrets(repo_url: String, secrets: HashMap<String, String>) -> Result<(), String> {
    let repo_key = stable_id(&repo_url);
    let mut current = read_secret_store(&repo_key)?;
    for (key, value) in secrets {
        if !value.trim().is_empty() {
            current.insert(key, protect_secret(&value)?);
        }
    }
    let path = secrets_path(&repo_key);
    ensure_parent(&path)?;
    fs::write(path, serde_json::to_string_pretty(&current).map_err(display_err)?).map_err(display_err)
}

#[tauri::command]
fn get_tool_status() -> Result<ToolStatusResponse, String> {
    let status = tools::tool_status();
    Ok(ToolStatusResponse {
        tools_root: status.tools_root,
        git: status.git,
        java: status.java,
        android_sdk: status.android_sdk,
        git_bash: status.git_bash,
        node: status.node,
        rust: status.rust,
        dotnet: status.dotnet,
    })
}

#[tauri::command]
fn install_build_tools(app: AppHandle) -> Result<ToolStatusResponse, String> {
    log(&app, "tools", "group", "Preparing local build tools");
    tools::ensure_git(|level, message| log(&app, "tools", level, message))?;
    let java = tools::ensure_java_version("17", |level, message| log(&app, "tools", level, message))?;
    tools::ensure_android_sdk(&[36], &java, |level, message| log(&app, "tools", level, message))?;
    log(&app, "tools", "success", "Local build tools are ready");
    get_tool_status()
}

#[tauri::command]
fn analyze_build(app: AppHandle, request: BuildRequest) -> Result<BuildPlan, String> {
    let source = resolve_source(&app, &request, true)?;
    analyze_request(&source, &request)
}

#[tauri::command]
fn install_plan_tools(app: AppHandle, request: BuildRequest) -> Result<ToolStatusResponse, String> {
    let source = resolve_source(&app, &request, true)?;
    let plan = analyze_request(&source, &request)?;
    if !plan.supported {
        return Err(format!("Resolve the build-plan blockers before installing tools: {}", plan.blockers.join(" ")));
    }
    log(&app, "tools", "group", &format!("Preparing tools for {}", plan.target_label));
    let compile_sdks = infer_compile_sdks(&source);
    tools::ensure_plan_tools(&plan.tools, &compile_sdks, |level, message| log(&app, "tools", level, message))?;
    log(&app, "tools", "success", "Required local build tools are ready");
    get_tool_status()
}

#[tauri::command]
fn list_build_history(output_folder: String, source_name: String) -> Result<Vec<BuildManifest>, String> {
    let source_root = PathBuf::from(output_folder).join(sanitize(&source_name));
    artifacts::read_history(&source_root, 25)
}

#[tauri::command]
fn open_path(path: String) -> Result<(), String> {
    let target = PathBuf::from(path);
    if !target.exists() {
        return Err(format!("Path does not exist: {}", target.display()));
    }
    #[cfg(windows)]
    {
        Command::new("explorer.exe")
            .arg(&target)
            .spawn()
            .map_err(display_err)?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = target;
        Err("Open folder is currently supported on Windows.".to_string())
    }
}

#[tauri::command]
fn run_build(app: AppHandle, state: State<BuildState>, request: BuildRequest) -> Result<BuildResult, String> {
    let build_id = format!("build-{}", Local::now().format("%Y%m%d%H%M%S"));
    {
        let mut running = state.running.lock().map_err(|_| "Build state lock poisoned".to_string())?;
        if running.is_some() {
            return Err("A build is already running".to_string());
        }
        *running = Some(build_id.clone());
    }
    state.cancel.store(false, Ordering::SeqCst);

    let outcome = run_build_inner(app.clone(), state.cancel.clone(), build_id.clone(), request);
    let mut running = state.running.lock().map_err(|_| "Build state lock poisoned".to_string())?;
    *running = None;
    outcome
}

#[tauri::command]
fn cancel_build(state: State<BuildState>) -> Result<(), String> {
    state.cancel.store(true, Ordering::SeqCst);
    Ok(())
}

fn run_build_inner(app: AppHandle, cancel: Arc<AtomicBool>, build_id: String, request: BuildRequest) -> Result<BuildResult, String> {
    log(&app, &build_id, "group", "Preparing source and local build plan");
    let source = resolve_source(&app, &request, true)?;
    let plan = analyze_request(&source, &request)?;
    if !plan.supported {
        return Err(format!("Build plan has blockers: {}", plan.blockers.join(" ")));
    }
    let compile_sdks = infer_compile_sdks(&source);
    let installed = tools::ensure_plan_tools(
        &plan.tools,
        &compile_sdks,
        |level, message| log(&app, &build_id, level, message),
    )?;
    let source_key = source_key(&request);
    let secrets = unprotect_secret_store(&stable_id(&source_key))?;
    let mut missing = plan
        .required_secrets
        .iter()
        .filter(|name| !secrets.contains_key(*name))
        .cloned()
        .collect::<Vec<_>>();
    missing.sort();
    if !missing.is_empty() {
        return Err(format!("Missing local secrets: {}. Add them before building.", missing.join(", ")));
    }

    let workspaces_root = default_app_data().join("ApkBuildLauncher").join("workspaces");
    let workspace = IsolatedWorkspace::create(&source, &workspaces_root, &build_id)?;
    log(&app, &build_id, "success", &format!("Isolated workspace ready at {}", workspace.path().display()));
    let source_name = source_display_name(&request, &source);
    let final_output = output_folder_for_source(
        &request.output_folder,
        &source_name,
        &request.ref_name,
    )?;
    let revision = source_revision(&source).unwrap_or_else(|| "local-uncommitted".to_string());
    let mut base_env = installed.env;
    base_env.extend(value_map_to_strings(&plan.env));
    if matches!(plan.target, BuildTarget::Android) {
        let gradle_home = default_app_data().join("ApkBuildLauncher").join("gradle");
        ensure_dir(&gradle_home)?;
        base_env.insert("GRADLE_USER_HOME".to_string(), gradle_home.display().to_string());
        base_env.insert(
            "GRADLE_OPTS".to_string(),
            "-Dorg.gradle.caching=true -Dorg.gradle.parallel=true -Dorg.gradle.daemon=true".to_string(),
        );
    }
    base_env.insert("GITHUB_WORKSPACE".to_string(), workspace.path().display().to_string());
    base_env.insert("GITHUB_REF_NAME".to_string(), request.ref_name.clone());
    let sensitive_values = secrets.values().cloned().collect::<Vec<_>>();

    for (index, step) in plan.steps.iter().enumerate() {
        if cancel.load(Ordering::SeqCst) {
            return Err("Build cancelled".to_string());
        }
        log(&app, &build_id, "group", &format!("Step {}: {}", index + 1, step.name));
        match step.disposition {
            StepDisposition::Skip | StepDisposition::Setup | StepDisposition::Collect => {
                log(&app, &build_id, "info", &step.reason);
            }
            StepDisposition::Block => return Err(format!("{}: {}", step.name, step.reason)),
            StepDisposition::Run => {
                let mut env = base_env.clone();
                for (key, value) in &step.env {
                    let raw = planning::value_to_string(value).unwrap_or_default();
                    let rendered = render_plan_text(&raw, workspace.path(), &request.ref_name, &secrets, &env)?;
                    env.insert(key.clone(), rendered);
                }
                let command = render_plan_text(
                    step.command.as_deref().unwrap_or_default(),
                    workspace.path(),
                    &request.ref_name,
                    &secrets,
                    &env,
                )?;
                let cwd = safe_working_directory(workspace.path(), step.working_directory.as_deref())?;
                let outcome = run_planned_process(
                    &app,
                    &build_id,
                    &cwd,
                    &command,
                    step.shell.as_deref(),
                    &request.shell_mode,
                    &env,
                    &sensitive_values,
                    cancel.clone(),
                );
                if let Err(error) = outcome {
                    if step.continue_on_error {
                        log(&app, &build_id, "warn", &format!("{} failed but is allowed to continue: {}", step.name, error));
                    } else {
                        return Err(error);
                    }
                }
            }
        }
        log(&app, &build_id, "success", &format!("Finished {}", step.name));
    }

    let published = artifacts::collect(workspace.path(), &final_output, &plan.artifacts)?;
    if published.is_empty() {
        return Err(format!(
            "Build commands completed, but none of the expected {} artifacts were produced.",
            plan.target_label
        ));
    }
    let manifest = BuildManifest {
        build_id: build_id.clone(),
        source: source_name.clone(),
        revision,
        target: plan.target_label.clone(),
        created_at: Local::now().to_rfc3339(),
        output_folder: final_output.display().to_string(),
        log_file: Some(log_file_path(&build_id).display().to_string()),
        artifacts: published.clone(),
    };
    let manifest_path = artifacts::write_manifest(&final_output, &manifest)?;
    let latest = PathBuf::from(&request.output_folder).join(sanitize(&source_name)).join("latest");
    artifacts::publish_latest(&final_output, &latest)?;
    workspace.cleanup()?;
    log(&app, &build_id, "success", &format!("Published {} artifact(s) to {}", published.len(), final_output.display()));
    Ok(BuildResult {
        build_id,
        output_folder: final_output.display().to_string(),
        target: plan.target,
        artifacts: published,
        manifest_path: manifest_path.display().to_string(),
    })
}

fn resolve_source(app: &AppHandle, request: &BuildRequest, update_remote: bool) -> Result<PathBuf, String> {
    match request.source_mode {
        SourceMode::Local => workspace::validate_local_source(&request.local_path),
        SourceMode::Remote => {
            if request.repo_url.trim().is_empty() {
                return Err("Enter a Git repository URL or choose a local project.".to_string());
            }
            tools::ensure_git(|level, message| log(app, "tools", level, message))?;
            prepare_repo_inner(request.repo_url.clone(), request.ref_name.clone(), update_remote).map(PathBuf::from)
        }
    }
}

fn analyze_request(source: &Path, request: &BuildRequest) -> Result<BuildPlan, String> {
    let workflow = if request.workflow_path.trim().is_empty() {
        None
    } else {
        let path = PathBuf::from(&request.workflow_path);
        let resolved = if path.is_absolute() { path } else { source.join(path) };
        Some(resolved)
    };
    planning::analyze(
        source,
        workflow.as_deref(),
        (!request.job_id.trim().is_empty()).then_some(request.job_id.as_str()),
        request.target,
    )
}

fn source_key(request: &BuildRequest) -> String {
    match request.source_mode {
        SourceMode::Remote => request.repo_url.trim().to_string(),
        SourceMode::Local => request.local_path.trim().to_string(),
    }
}

fn source_display_name(request: &BuildRequest, source: &Path) -> String {
    match request.source_mode {
        SourceMode::Remote => repo_name(&request.repo_url),
        SourceMode::Local => source
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("local-project")
            .to_string(),
    }
}

fn source_revision(source: &Path) -> Option<String> {
    run_output(git_spec(["rev-parse", "HEAD"]).cwd(source), None)
        .ok()
        .map(|revision| revision.trim().to_string())
        .filter(|revision| !revision.is_empty())
}

fn output_folder_for_source(root: &str, source_name: &str, ref_name: &str) -> Result<PathBuf, String> {
    let timestamp = Local::now().format("%Y%m%d-%H%M%S").to_string();
    let reference = if ref_name.trim().is_empty() { "local" } else { ref_name };
    let path = PathBuf::from(root)
        .join(sanitize(source_name))
        .join(sanitize(reference))
        .join(timestamp);
    ensure_dir(&path)?;
    Ok(path)
}

fn safe_working_directory(workspace: &Path, relative: Option<&str>) -> Result<PathBuf, String> {
    let candidate = relative
        .filter(|value| !value.trim().is_empty())
        .map(|value| workspace.join(value))
        .unwrap_or_else(|| workspace.to_path_buf());
    let canonical_workspace = workspace.canonicalize().map_err(display_err)?;
    let canonical = candidate
        .canonicalize()
        .map_err(|err| format!("Working directory '{}' is unavailable: {}", candidate.display(), err))?;
    if !canonical.starts_with(&canonical_workspace) {
        return Err(format!("Working directory escapes the isolated workspace: {}", relative.unwrap_or_default()));
    }
    Ok(canonical)
}

fn render_plan_text(
    input: &str,
    workspace: &Path,
    ref_name: &str,
    secrets: &HashMap<String, String>,
    env: &HashMap<String, String>,
) -> Result<String, String> {
    let mut output = input.to_string();
    for name in required_secrets_in_text(input) {
        let value = secrets.get(&name).ok_or_else(|| format!("Missing secret {}", name))?;
        for expression in [
            format!("${{{{ secrets.{} }}}}", name),
            format!("${{{{secrets.{}}}}}", name),
        ] {
            output = output.replace(&expression, value);
        }
    }
    for (key, value) in env {
        for expression in [
            format!("${{{{ env.{} }}}}", key),
            format!("${{{{env.{}}}}}", key),
        ] {
            output = output.replace(&expression, value);
        }
    }
    output = output
        .replace("${{ github.workspace }}", workspace.to_string_lossy().as_ref())
        .replace("${{github.workspace}}", workspace.to_string_lossy().as_ref())
        .replace("${{ github.ref_name }}", ref_name)
        .replace("${{github.ref_name}}", ref_name);
    if output.contains("${{") {
        return Err(format!("Unsupported workflow expression remains in command: {}", redact_line(&output)));
    }
    Ok(output)
}

#[allow(clippy::too_many_arguments)]
fn run_planned_process(
    app: &AppHandle,
    build_id: &str,
    cwd: &Path,
    script: &str,
    step_shell: Option<&str>,
    selected_shell: &ShellMode,
    envs: &HashMap<String, String>,
    sensitive_values: &[String],
    cancel: Arc<AtomicBool>,
) -> Result<(), String> {
    if matches!(selected_shell, ShellMode::Native) && script.contains("./gradlew") {
        let translated = script.replace("./gradlew", ".\\gradlew.bat");
        return spawn_planned_process(
            app,
            build_id,
            cwd,
            "powershell.exe",
            &["-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &translated],
            envs,
            sensitive_values,
            cancel,
        );
    }
    let requested = step_shell.unwrap_or_default().to_ascii_lowercase();
    let wants_bash = matches!(selected_shell, ShellMode::Bash)
        || requested.contains("bash")
        || requested.contains("sh")
        || looks_like_bash(script);
    if wants_bash {
        let bash = tools::find_git_bash()
            .ok_or_else(|| "This step needs Git Bash. Use Install / Repair Tools and retry.".to_string())?;
        spawn_planned_process(
            app,
            build_id,
            cwd,
            bash.to_string_lossy().as_ref(),
            &["-lc", script],
            envs,
            sensitive_values,
            cancel,
        )
    } else {
        spawn_planned_process(
            app,
            build_id,
            cwd,
            "powershell.exe",
            &["-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", script],
            envs,
            sensitive_values,
            cancel,
        )
    }
}

fn looks_like_bash(script: &str) -> bool {
    ["sed ", "mkdir -p", "cp ", "mv ", "find ", "base64 --decode", "#!/bin/"]
        .iter()
        .any(|marker| script.contains(marker))
}

#[allow(clippy::too_many_arguments)]
fn spawn_planned_process(
    app: &AppHandle,
    build_id: &str,
    cwd: &Path,
    program: &str,
    args: &[&str],
    envs: &HashMap<String, String>,
    sensitive_values: &[String],
    cancel: Arc<AtomicBool>,
) -> Result<(), String> {
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in envs {
        command.env(key, value);
    }
    let mut child = command
        .spawn()
        .map_err(|err| format!("Failed to start '{}': {}", program, err))?;
    let process_id = child.id();
    let stdout = child.stdout.take().ok_or_else(|| "Could not capture stdout".to_string())?;
    let stderr = child.stderr.take().ok_or_else(|| "Could not capture stderr".to_string())?;
    let app_out = app.clone();
    let id_out = build_id.to_string();
    let out_secrets = sensitive_values.to_vec();
    let app_err = app.clone();
    let id_err = build_id.to_string();
    let err_secrets = sensitive_values.to_vec();
    let out_thread = thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            log(&app_out, &id_out, "info", &redact_values(&line, &out_secrets));
        }
    });
    let err_thread = thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            log(&app_err, &id_err, "warn", &redact_values(&line, &err_secrets));
        }
    });
    let status = loop {
        if cancel.load(Ordering::SeqCst) {
            terminate_process_tree(process_id);
            let _ = child.kill();
            let _ = child.wait();
            return Err("Build cancelled".to_string());
        }
        if let Some(status) = child.try_wait().map_err(display_err)? {
            break status;
        }
        thread::sleep(std::time::Duration::from_millis(200));
    };
    let _ = out_thread.join();
    let _ = err_thread.join();
    if status.success() {
        Ok(())
    } else {
        Err(format!("{} failed with status {}. See the build log for command output.", program, status))
    }
}

fn terminate_process_tree(process_id: u32) {
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill.exe")
            .args(["/PID", &process_id.to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

fn redact_values(line: &str, sensitive_values: &[String]) -> String {
    let mut output = redact_line(line);
    for value in sensitive_values.iter().filter(|value| !value.is_empty()) {
        output = output.replace(value, "[redacted secret]");
    }
    output
}

fn run_step(app: &AppHandle, build_id: &str, request: &BuildRequest, repo_path: &Path, context: &Context, step: &StepDoc, cancel: Arc<AtomicBool>) -> Result<(), String> {
    if let Some(uses) = &step.uses {
        let lower = uses.to_ascii_lowercase();
        if lower.starts_with("actions/checkout@") {
            log(app, build_id, "info", "actions/checkout is already satisfied by the local clone");
            return Ok(());
        }
        if lower.starts_with("actions/setup-java@") {
            let version = step.with.get("java-version").and_then(value_to_string).unwrap_or_else(|| "17".to_string());
            let java = tools::ensure_java_version(&version, |level, message| log(app, build_id, level, message))?;
            log(app, build_id, "success", &format!("Java {} is available at {}", version, java.java.display()));
            return Ok(());
        }
        if lower.starts_with("actions/upload-artifact@") {
            let path = step.with.get("path").and_then(value_to_string).ok_or_else(|| "upload-artifact requires with.path".to_string())?;
            let output = output_folder(&request.output_folder, &request.repo_url, &request.ref_name)?;
            copy_artifacts(repo_path, &path, &output)?;
            log(app, build_id, "success", "Artifact step copied files locally");
            return Ok(());
        }
        return Err(format!("Unsupported action '{}'. v1 supports checkout, setup-java, and upload-artifact.", uses));
    }

    let Some(run) = &step.run else {
        log(app, build_id, "warn", "Step has neither uses nor run; nothing to execute");
        return Ok(());
    };
    let env = context.env_for_step(step)?;
    let script = replace_expressions(run, context, &env)?;

    if matches!(request.shell_mode, ShellMode::Native) {
        if run_native_translation(app, build_id, repo_path, &script, &env)? {
            return Ok(());
        }
        if script.contains("./gradlew") {
            let translated = script.replace("./gradlew", ".\\gradlew.bat");
            return run_process(app, build_id, repo_path, "powershell.exe", &["-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &translated], &env, cancel);
        }
    }

    let bash = find_git_bash().ok_or_else(|| "Complex bash script requires Git Bash. Install Git for Windows or switch to a simpler Native Windows-compatible workflow step.".to_string())?;
    run_process(app, build_id, repo_path, bash.to_string_lossy().as_ref(), &["-lc", &script], &env, cancel)
}

fn run_native_translation(app: &AppHandle, build_id: &str, repo_path: &Path, script: &str, env: &HashMap<String, String>) -> Result<bool, String> {
    let compact = script.replace("\r\n", "\n");
    if compact.contains("base64 --decode > local.properties") {
        decode_property_secret(repo_path, env, "LOCAL_PROPERTIES_BASE64", "local.properties")?;
        decode_property_secret(repo_path, env, "LOCAL_DEV_PROPERTIES_BASE64", "local.dev.properties")?;
        log(app, build_id, "success", "Local property files prepared without logging their contents");
        return Ok(true);
    }

    if compact.contains("sed -i") && compact.contains("app/build.gradle.kts") {
        let gradle_path = repo_path.join("app").join("build.gradle.kts");
        let mut text = fs::read_to_string(&gradle_path).map_err(display_err)?;
        text = text.replace("applicationId = \"com.nuvio.tv\"", "applicationId = \"com.nuvio.tv.harrybarnes\"");
        text = text.replace("isEnable = !buildingAppBundle", "isEnable = false");
        text = replace_version_name(&text);
        fs::write(&gradle_path, text).map_err(display_err)?;
        log(app, build_id, "success", "Applied Android build configuration updates");
        return Ok(true);
    }

    if compact.contains("mkdir -p build/outputs/harrybarnes-apks") && compact.contains("find app/build/outputs/apk") {
        let target = repo_path.join("build").join("outputs").join("harrybarnes-apks");
        ensure_dir(&target)?;
        for apk in find_apks(&repo_path.join("app").join("build").join("outputs").join("apk"))? {
            let stem = apk.file_stem().unwrap_or_default().to_string_lossy();
            let destination = target.join(format!("{}-harrybarnes.apk", stem));
            fs::rename(&apk, &destination).or_else(|_| {
                fs::copy(&apk, &destination).map(|_| ())
            }).map_err(display_err)?;
        }
        log(app, build_id, "success", "Renamed APK files into build/outputs/harrybarnes-apks");
        return Ok(true);
    }

    Ok(false)
}

fn run_process(app: &AppHandle, build_id: &str, cwd: &Path, program: &str, args: &[&str], envs: &HashMap<String, String>, cancel: Arc<AtomicBool>) -> Result<(), String> {
    let mut command = Command::new(program);
    command.args(args).current_dir(cwd).stdout(Stdio::piped()).stderr(Stdio::piped());
    for (key, value) in envs {
        command.env(key, value);
    }
    let mut child = command.spawn().map_err(|err| format!("Failed to start '{}': {}", program, err))?;
    let stdout = child.stdout.take().ok_or_else(|| "Could not capture stdout".to_string())?;
    let stderr = child.stderr.take().ok_or_else(|| "Could not capture stderr".to_string())?;
    let app_out = app.clone();
    let id_out = build_id.to_string();
    let app_err = app.clone();
    let id_err = build_id.to_string();

    let out_thread = thread::spawn(move || {
        for line in BufReader::new(stdout).lines().flatten() {
            log(&app_out, &id_out, "info", &redact_line(&line));
        }
    });
    let err_thread = thread::spawn(move || {
        for line in BufReader::new(stderr).lines().flatten() {
            log(&app_err, &id_err, "warn", &redact_line(&line));
        }
    });

    let status = loop {
        if cancel.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Build cancelled".to_string());
        }
        if let Some(status) = child.try_wait().map_err(display_err)? {
            break status;
        }
        thread::sleep(std::time::Duration::from_millis(200));
    };
    let _ = out_thread.join();
    let _ = err_thread.join();
    if status.success() {
        Ok(())
    } else {
        Err(format!("Command failed with status {}: {} {}", status, program, args.join(" ")))
    }
}

#[derive(Clone)]
struct Context {
    workspace: PathBuf,
    ref_name: String,
    android_sdk: PathBuf,
    java_home: Option<PathBuf>,
    java_bin_dir: Option<PathBuf>,
    workflow_env: HashMap<String, String>,
    job_env: HashMap<String, String>,
    secrets: HashMap<String, String>,
}

impl Context {
    fn env_for_step(&self, step: &StepDoc) -> Result<HashMap<String, String>, String> {
        let mut env = self.workflow_env.clone();
        env.extend(self.job_env.clone());
        let android_sdk = self.android_sdk.to_string_lossy().to_string();
        env.entry("ANDROID_HOME".to_string()).or_insert_with(|| android_sdk.clone());
        env.entry("ANDROID_SDK_ROOT".to_string()).or_insert(android_sdk);
        let gradle_home = default_gradle_user_home();
        ensure_dir(&gradle_home)?;
        env.entry("GRADLE_USER_HOME".to_string()).or_insert_with(|| gradle_home.to_string_lossy().to_string());
        merge_env_value(
            &mut env,
            "GRADLE_OPTS",
            "-Dorg.gradle.caching=true -Dorg.gradle.parallel=true -Dorg.gradle.daemon=true",
        );
        if let Some(java_home) = &self.java_home {
            env.entry("JAVA_HOME".to_string()).or_insert_with(|| java_home.to_string_lossy().to_string());
        }
        if let Some(java_bin_dir) = &self.java_bin_dir {
            let existing_path = std::env::var("PATH").unwrap_or_default();
            let separator = if cfg!(windows) { ";" } else { ":" };
            env.entry("PATH".to_string()).or_insert_with(|| format!("{}{}{}", java_bin_dir.display(), separator, existing_path));
        }
        for (key, value) in &step.env {
            let raw = value_to_string(value).unwrap_or_default();
            let replaced = replace_expressions(&raw, self, &env)?;
            env.insert(key.clone(), replaced);
        }
        Ok(env)
    }
}

fn replace_expressions(input: &str, context: &Context, env: &HashMap<String, String>) -> Result<String, String> {
    let mut output = input.to_string();
    for name in required_secrets_in_text(input) {
        let value = context.secrets.get(&name).ok_or_else(|| format!("Missing secret {}", name))?;
        output = output.replace(&format!("${{{{ secrets.{} }}}}", name), value);
    }
    for (key, value) in env {
        output = output.replace(&format!("${{{{ env.{} }}}}", key), value);
    }
    output = output.replace("${{ github.workspace }}", context.workspace.to_string_lossy().as_ref());
    output = output.replace("${{ github.ref_name }}", &context.ref_name);
    Ok(output)
}

fn required_secrets(doc: &WorkflowDoc, job: &JobDoc) -> HashSet<String> {
    let mut names = HashSet::new();
    for value in doc.env.values().chain(job.env.values()) {
        if let Some(text) = value_to_string(value) {
            names.extend(required_secrets_in_text(&text));
        }
    }
    for step in &job.steps {
        if let Some(run) = &step.run {
            names.extend(required_secrets_in_text(run));
        }
        for value in step.env.values().chain(step.with.values()) {
            if let Some(text) = value_to_string(value) {
                names.extend(required_secrets_in_text(&text));
            }
        }
    }
    names
}

fn required_secrets_in_text(text: &str) -> HashSet<String> {
    let mut names = HashSet::new();
    let marker = "${{ secrets.";
    let mut rest = text;
    while let Some(start) = rest.find(marker) {
        let after = &rest[start + marker.len()..];
        if let Some(end) = after.find(" }}") {
            names.insert(after[..end].trim().to_string());
            rest = &after[end + 3..];
        } else if let Some(end) = after.find("}}") {
            names.insert(after[..end].trim().to_string());
            rest = &after[end + 2..];
        } else {
            break;
        }
    }
    names
}

fn requested_java_version(job: &JobDoc) -> String {
    job.steps
        .iter()
        .find_map(|step| {
            let uses = step.uses.as_ref()?.to_ascii_lowercase();
            if !uses.starts_with("actions/setup-java@") {
                return None;
            }
            step.with.get("java-version").and_then(value_to_string)
        })
        .unwrap_or_else(|| "17".to_string())
}

fn infer_compile_sdks(repo_path: &Path) -> Vec<u32> {
    let mut versions = HashSet::new();
    collect_compile_sdks(repo_path, &mut versions, 0);
    let mut versions = versions.into_iter().collect::<Vec<_>>();
    versions.sort();
    versions
}

fn collect_compile_sdks(path: &Path, versions: &mut HashSet<u32>, depth: usize) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
        if path.is_dir() {
            if matches!(name, ".git" | "build" | ".gradle") {
                continue;
            }
            collect_compile_sdks(&path, versions, depth + 1);
            continue;
        }
        if !matches!(name, "build.gradle" | "build.gradle.kts") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        for line in text.lines() {
            let trimmed = line.trim();
            if !trimmed.starts_with("compileSdk") {
                continue;
            }
            if let Some(version) = trimmed
                .chars()
                .skip_while(|c| !c.is_ascii_digit())
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse::<u32>()
                .ok()
            {
                versions.insert(version);
            }
        }
    }
}

fn load_workflow(path: &str) -> Result<WorkflowDoc, String> {
    let text = fs::read_to_string(path).map_err(display_err)?;
    serde_yaml::from_str(&text).map_err(display_err)
}

fn checkout_ref(repo_path: &Path, ref_name: &str) -> Result<(), String> {
    let trimmed = ref_name.trim();
    if trimmed.chars().all(|c| c.is_ascii_digit()) {
        let pr_ref = format!("pull/{}/head:pr-{}", trimmed, trimmed);
        run_checked(git_spec(["fetch", "origin", &pr_ref]).cwd(repo_path), None)?;
        run_checked(git_spec(["checkout", &format!("pr-{}", trimmed)]).cwd(repo_path), None)
    } else {
        let remote_ref = format!("origin/{}", trimmed);
        let checkout_target = if run_checked(git_spec(["rev-parse", "--verify", &remote_ref]).cwd(repo_path), None).is_ok() {
            remote_ref
        } else {
            trimmed.to_string()
        };
        run_checked(git_spec(["checkout", &checkout_target]).cwd(repo_path), None)
    }
}

fn repo_matches_ref(repo_path: &Path, ref_name: &str) -> bool {
    let trimmed = ref_name.trim();
    if trimmed.is_empty() {
        return false;
    }

    let current_branch = run_output(git_spec(["rev-parse", "--abbrev-ref", "HEAD"]).cwd(repo_path), None).unwrap_or_default();
    let current_branch = current_branch.trim();
    let pr_branch = format!("pr-{}", trimmed);
    if current_branch == trimmed || (trimmed.chars().all(|c| c.is_ascii_digit()) && current_branch == pr_branch.as_str()) {
        return true;
    }

    let Ok(head) = run_output(git_spec(["rev-parse", "HEAD"]).cwd(repo_path), None) else {
        return false;
    };
    let target = if trimmed.chars().all(|c| c.is_ascii_digit()) {
        format!("pr-{}", trimmed)
    } else {
        format!("origin/{}", trimmed)
    };
    let Ok(target_head) = run_output(git_spec(["rev-parse", &target]).cwd(repo_path), None) else {
        return false;
    };
    head.trim() == target_head.trim()
}

fn parse_remote_branches(output: &str) -> Vec<String> {
    let mut default_branch = None;
    let mut branches = vec![];
    for line in output.lines() {
        if let Some(rest) = line.strip_prefix("ref: refs/heads/") {
            default_branch = rest.split_whitespace().next().map(|branch| branch.to_string());
            continue;
        }
        let Some((_, reference)) = line.split_once('\t') else {
            continue;
        };
        if let Some(branch) = reference.strip_prefix("refs/heads/") {
            branches.push(branch.to_string());
        }
    }
    branches.sort();
    branches.dedup();
    if let Some(default_branch) = default_branch {
        if let Some(index) = branches.iter().position(|branch| branch == &default_branch) {
            let branch = branches.remove(index);
            branches.insert(0, branch);
        }
    }
    branches
}

#[derive(Clone)]
struct JavaInstall {
    java: PathBuf,
    home: Option<PathBuf>,
}

fn validate_tools(needs_bash: bool) -> Result<JavaInstall, String> {
    validate_git()?;
    let java = validate_java_version("17")?;
    if needs_bash && find_git_bash().is_none() {
        return Err("Git Bash is required for Bash compatibility mode. Install Git for Windows for the current user, or place portable Git at %LOCALAPPDATA%\\ApkBuildLauncher\\tools\\Git.".to_string());
    }
    Ok(java)
}

fn validate_git() -> Result<(), String> {
    run_checked(git_spec(["--version"]), None).map_err(|_| "Git is required for cloning and branch lookup. Install Git for Windows with the per-user installer, ensure git is in PATH, or place portable Git at %LOCALAPPDATA%\\ApkBuildLauncher\\tools\\Git.".to_string())
}

fn validate_java_version(expected: &str) -> Result<JavaInstall, String> {
    let expected_major = expected_java_major(expected);
    let mut detected = vec![];
    for (java, home) in java_candidates(expected_major.as_deref()) {
        let output = Command::new(&java).arg("-version").output();
        let Ok(output) = output else {
            continue;
        };
        let version_text = String::from_utf8_lossy(&output.stderr).to_string() + &String::from_utf8_lossy(&output.stdout);
        let major = java_major_version(&version_text).unwrap_or_else(|| "unknown".to_string());
        if expected_major.as_deref().map_or(true, |expected| major == expected) {
            return Ok(JavaInstall { java, home });
        }
        detected.push(format!("{} at {}", major, java.display()));
    }

    let expected_text = expected_major.as_deref().unwrap_or(expected);
    if detected.is_empty() {
        Err(format!("Java {} is required for building, but Java was not found. Install Temurin/OpenJDK {} with a per-user installer, set JAVA_HOME, or unpack a JDK to %LOCALAPPDATA%\\ApkBuildLauncher\\tools\\jdk-{}. No admin rights are needed for that folder.", expected_text, expected_text, expected_text))
    } else {
        Err(format!("Java {} is required for building, but detected Java {}. Install Temurin/OpenJDK {} with a per-user installer, set JAVA_HOME to it, or unpack a JDK to %LOCALAPPDATA%\\ApkBuildLauncher\\tools\\jdk-{}. No admin rights are needed for that folder.", expected_text, detected.join(", "), expected_text, expected_text))
    }
}

fn check_android_sdk() -> Result<(), String> {
    android_sdk_path().map(|_| ())
}

fn android_sdk_path() -> Result<PathBuf, String> {
    for name in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Ok(value) = std::env::var(name) {
            let path = PathBuf::from(value);
            if path.exists() {
                return Ok(path);
            }
        }
    }
    let fallback = dirs::data_local_dir().unwrap_or_else(default_app_data).join("Android").join("Sdk");
    if fallback.exists() {
        return Ok(fallback);
    }
    Err("Android SDK was not found. Set ANDROID_HOME or ANDROID_SDK_ROOT, or install it at %LOCALAPPDATA%\\Android\\Sdk.".to_string())
}

fn decode_property_secret(repo_path: &Path, env: &HashMap<String, String>, name: &str, file_name: &str) -> Result<(), String> {
    if let Some(value) = env.get(name).filter(|value| !value.is_empty()) {
        let decoded = base64::engine::general_purpose::STANDARD.decode(value).map_err(|_| format!("{} is not valid base64", name))?;
        fs::write(repo_path.join(file_name), decoded).map_err(display_err)?;
    }
    Ok(())
}

fn replace_version_name(text: &str) -> String {
    text.lines()
        .map(|line| {
            if line.contains("versionName = \"") && !line.contains("-harrybarnes\"") {
                let mut out = line.to_string();
                if let Some(last_quote) = out.rfind('"') {
                    out.insert_str(last_quote, "-harrybarnes");
                }
                out
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn copy_artifacts(repo_path: &Path, pattern: &str, output: &Path) -> Result<(), String> {
    ensure_dir(output)?;
    let full_pattern = repo_path.join(pattern).to_string_lossy().replace('\\', "/");
    for entry in glob(&full_pattern).map_err(display_err)? {
        let path = entry.map_err(display_err)?;
        if path.is_file() {
            let destination = output.join(path.file_name().unwrap_or_default());
            fs::copy(&path, destination).map_err(display_err)?;
        }
    }
    Ok(())
}

fn copy_apks(repo_path: &Path, output: &Path) -> Result<Vec<PathBuf>, String> {
    ensure_dir(output)?;
    let preferred = repo_path.join("build").join("outputs").join("harrybarnes-apks");
    let search_root = if preferred.exists() { preferred } else { repo_path.join("app").join("build").join("outputs").join("apk") };
    let mut copied = vec![];
    for apk in find_apks(&search_root)? {
        let destination = output.join(apk.file_name().unwrap_or_default());
        fs::copy(&apk, &destination).map_err(display_err)?;
        copied.push(destination);
    }
    Ok(copied)
}

fn sync_latest(final_output: &Path, root_output: &str, repo_url: &str, copied: &[PathBuf]) -> Result<(), String> {
    let latest = PathBuf::from(root_output).join(repo_name(repo_url)).join("latest");
    ensure_dir(&latest)?;
    for apk in find_apks(&latest)? {
        fs::remove_file(apk).map_err(display_err)?;
    }
    for apk in copied {
        let destination = latest.join(apk.file_name().unwrap_or_default());
        fs::copy(final_output.join(apk.file_name().unwrap_or_default()), destination).map_err(display_err)?;
    }
    Ok(())
}

fn find_apks(root: &Path) -> Result<Vec<PathBuf>, String> {
    if !root.exists() {
        return Ok(vec![]);
    }
    let pattern = root.join("**").join("*.apk").to_string_lossy().replace('\\', "/");
    let mut files = vec![];
    for entry in glob(&pattern).map_err(display_err)? {
        let path = entry.map_err(display_err)?;
        if path.is_file() {
            files.push(path);
        }
    }
    Ok(files)
}

fn output_folder(root: &str, repo_url: &str, ref_name: &str) -> Result<PathBuf, String> {
    let timestamp = Local::now().format("%Y%m%d-%H%M%S").to_string();
    let safe_ref = sanitize(ref_name);
    let path = PathBuf::from(root).join(repo_name(repo_url)).join(safe_ref).join(timestamp);
    ensure_dir(&path)?;
    Ok(path)
}

fn protect_local_properties(repo_path: &Path) -> Result<(), String> {
    let exclude = repo_path.join(".git").join("info").join("exclude");
    ensure_parent(&exclude)?;
    let mut text = fs::read_to_string(&exclude).unwrap_or_default();
    for line in ["local.properties", "local.dev.properties"] {
        if !text.lines().any(|existing| existing.trim() == line) {
            text.push('\n');
            text.push_str(line);
        }
    }
    fs::write(exclude, text).map_err(display_err)
}

fn read_secret_store(repo_key: &str) -> Result<HashMap<String, String>, String> {
    let path = secrets_path(repo_key);
    if !path.exists() {
        return Ok(HashMap::new());
    }
    serde_json::from_str(&fs::read_to_string(path).map_err(display_err)?).map_err(display_err)
}

fn unprotect_secret_store(repo_key: &str) -> Result<HashMap<String, String>, String> {
    read_secret_store(repo_key)?
        .into_iter()
        .map(|(key, value)| unprotect_secret(&value).map(|plain| (key, plain)))
        .collect()
}

#[cfg(windows)]
fn protect_secret(value: &str) -> Result<String, String> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB},
    };
    let mut input = CRYPT_INTEGER_BLOB { cbData: value.as_bytes().len() as u32, pbData: value.as_ptr() as *mut u8 };
    let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    let ok = unsafe {
        CryptProtectData(
            &mut input,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 {
        return Err("Windows DPAPI failed while saving a secret".to_string());
    }
    let bytes = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) };
    let protected = format!("dpapi:{}", base64::engine::general_purpose::STANDARD.encode(bytes));
    unsafe {
        let _ = LocalFree(output.pbData as _);
    }
    Ok(protected)
}

#[cfg(windows)]
fn unprotect_secret(value: &str) -> Result<String, String> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB},
    };
    let encoded = value.strip_prefix("dpapi:").unwrap_or(value);
    let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).map_err(display_err)?;
    let mut input = CRYPT_INTEGER_BLOB { cbData: bytes.len() as u32, pbData: bytes.as_ptr() as *mut u8 };
    let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    let ok = unsafe {
        CryptUnprotectData(
            &mut input,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 {
        return Err("Windows DPAPI failed while reading a secret".to_string());
    }
    let bytes = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) };
    let plain = String::from_utf8(bytes.to_vec()).map_err(display_err);
    unsafe {
        let _ = LocalFree(output.pbData as _);
    }
    plain
}

#[cfg(not(windows))]
fn protect_secret(value: &str) -> Result<String, String> {
    Ok(base64::engine::general_purpose::STANDARD.encode(value))
}

#[cfg(not(windows))]
fn unprotect_secret(value: &str) -> Result<String, String> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(value).map_err(display_err)?;
    String::from_utf8(bytes).map_err(display_err)
}

struct CommandSpec {
    program: String,
    args: Vec<String>,
    cwd: Option<PathBuf>,
}

impl CommandSpec {
    fn new<I, S>(program: &str, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Self { program: program.to_string(), args: args.into_iter().map(|arg| arg.as_ref().to_string()).collect(), cwd: None }
    }

    fn new_path<I, S>(program: PathBuf, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Self { program: program.to_string_lossy().to_string(), args: args.into_iter().map(|arg| arg.as_ref().to_string()).collect(), cwd: None }
    }

    fn cwd(mut self, path: &Path) -> Self {
        self.cwd = Some(path.to_path_buf());
        self
    }
}

fn run_checked(spec: CommandSpec, envs: Option<&HashMap<String, String>>) -> Result<(), String> {
    let mut command = Command::new(&spec.program);
    command.args(&spec.args);
    if let Some(cwd) = spec.cwd {
        command.current_dir(cwd);
    }
    if let Some(envs) = envs {
        for (key, value) in envs {
            command.env(key, value);
        }
    }
    let output = command.output().map_err(display_err)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(redact_line(&String::from_utf8_lossy(&output.stderr)))
    }
}

fn run_output(spec: CommandSpec, envs: Option<&HashMap<String, String>>) -> Result<String, String> {
    let mut command = Command::new(&spec.program);
    command.args(&spec.args);
    if let Some(cwd) = spec.cwd {
        command.current_dir(cwd);
    }
    if let Some(envs) = envs {
        for (key, value) in envs {
            command.env(key, value);
        }
    }
    let output = command.output().map_err(display_err)?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(redact_line(&String::from_utf8_lossy(&output.stderr)))
    }
}

fn value_map_to_strings(map: &BTreeMap<String, Value>) -> HashMap<String, String> {
    map.iter().filter_map(|(key, value)| value_to_string(value).map(|text| (key.clone(), text))).collect()
}

fn merge_env_value(env: &mut HashMap<String, String>, key: &str, addition: &str) {
    env.entry(key.to_string())
        .and_modify(|current| {
            if !current.contains(addition) {
                if !current.trim().is_empty() {
                    current.push(' ');
                }
                current.push_str(addition);
            }
        })
        .or_insert_with(|| addition.to_string());
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

fn summarize_trigger(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Sequence(items) => items.iter().filter_map(value_to_string).collect::<Vec<_>>().join(", "),
        Value::Mapping(map) => map.keys().filter_map(value_to_string).collect::<Vec<_>>().join(", "),
        _ => "unknown".to_string(),
    }
}

fn runs_on_to_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Sequence(items) => items.iter().filter_map(value_to_string).collect::<Vec<_>>().join(", "),
        _ => "unspecified".to_string(),
    }
}

fn find_git_bash() -> Option<PathBuf> {
    tools::find_git_bash()
}

fn git_program() -> PathBuf {
    tools::git_program().unwrap_or_else(|| PathBuf::from("git"))
}

fn git_spec<I, S>(args: I) -> CommandSpec
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    CommandSpec::new_path(git_program(), args)
}

fn java_candidates(expected_major: Option<&str>) -> Vec<(PathBuf, Option<PathBuf>)> {
    let exe = if cfg!(windows) { "java.exe" } else { "java" };
    let mut candidates = vec![];
    if let Ok(java_home) = std::env::var("JAVA_HOME") {
        push_java_home(&mut candidates, PathBuf::from(java_home), exe);
    }
    if let Some(local_app_data) = dirs::data_local_dir() {
        let tool_root = local_app_data.join("ApkBuildLauncher").join("tools");
        if let Some(major) = expected_major {
            push_java_home(&mut candidates, tool_root.join(format!("jdk-{}", major)), exe);
        }
        push_matching_java_homes(&mut candidates, &tool_root, expected_major, exe);
        push_matching_java_homes(&mut candidates, &local_app_data.join("Programs").join("Eclipse Adoptium"), expected_major, exe);
        push_matching_java_homes(&mut candidates, &local_app_data.join("Programs").join("Java"), expected_major, exe);
    }
    for root in [r"C:\Program Files\Eclipse Adoptium", r"C:\Program Files\Java", r"C:\Program Files\Microsoft"] {
        push_matching_java_homes(&mut candidates, &PathBuf::from(root), expected_major, exe);
    }
    candidates.push((PathBuf::from(exe), None));
    candidates
}

fn push_java_home(candidates: &mut Vec<(PathBuf, Option<PathBuf>)>, home: PathBuf, exe: &str) {
    let java = home.join("bin").join(exe);
    if java.exists() {
        candidates.push((java, Some(home)));
    }
}

fn push_matching_java_homes(candidates: &mut Vec<(PathBuf, Option<PathBuf>)>, root: &Path, expected_major: Option<&str>, exe: &str) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    let mut homes = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter(|path| {
            let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default().to_ascii_lowercase();
            expected_major.map_or(true, |major| name.contains(&format!("jdk-{}", major)) || name.contains(&format!("jdk{}", major)) || name.contains(&format!("-{}", major)))
        })
        .collect::<Vec<_>>();
    homes.sort();
    for home in homes {
        push_java_home(candidates, home, exe);
    }
}

fn expected_java_major(expected: &str) -> Option<String> {
    let digits = expected.trim().chars().take_while(|c| c.is_ascii_digit()).collect::<String>();
    if digits.is_empty() {
        None
    } else {
        Some(digits)
    }
}

fn java_major_version(version_text: &str) -> Option<String> {
    let quoted = version_text.split('"').nth(1)?;
    let mut parts = quoted.split('.');
    let first = parts.next()?;
    if first == "1" {
        parts.next().map(|part| part.to_string())
    } else {
        Some(first.to_string())
    }
}

fn repo_path_for(repo_url: &str) -> Result<PathBuf, String> {
    Ok(configured_repo_root().join(format!("{}-{}", sanitize(&repo_name(repo_url)), &stable_id(repo_url)[..8])))
}

fn repo_name(repo_url: &str) -> String {
    repo_url.trim_end_matches(".git").rsplit(['/', ':']).next().unwrap_or("repo").to_string()
}

fn sanitize(input: &str) -> String {
    input.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '-' }).collect()
}

fn stable_id(input: &str) -> String {
    format!("{:x}", Sha256::digest(input.as_bytes()))
}

fn config_path() -> PathBuf {
    config_root().join("config.json")
}

fn secrets_path(repo_key: &str) -> PathBuf {
    config_root().join("secrets").join(format!("{}.json", repo_key))
}

fn config_root() -> PathBuf {
    dirs::config_dir().unwrap_or_else(default_app_data).join("ApkBuildLauncher")
}

fn default_repo_root() -> PathBuf {
    default_app_data().join("ApkBuildLauncher").join("repos")
}

fn default_gradle_user_home() -> PathBuf {
    default_app_data().join("ApkBuildLauncher").join("gradle")
}

fn configured_repo_root() -> PathBuf {
    get_config()
        .ok()
        .map(|config| PathBuf::from(config.default_repo_folder))
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(default_repo_root)
}

fn default_app_data() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."))
}

fn ensure_parent(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    Ok(())
}

fn ensure_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(display_err)
}

fn display_err<E: std::fmt::Display>(err: E) -> String {
    err.to_string()
}

fn redact_line(line: &str) -> String {
    if line.contains("LOCAL_PROPERTIES_BASE64") || line.contains("LOCAL_DEV_PROPERTIES_BASE64") || line.contains("local.properties") || line.contains("local.dev.properties") {
        "[redacted sensitive property output]".to_string()
    } else {
        line.to_string()
    }
}

fn log_file_path(build_id: &str) -> PathBuf {
    config_root().join("logs").join(format!("{}.log", sanitize(build_id)))
}

fn log(app: &AppHandle, build_id: &str, level: &str, message: &str) {
    let path = log_file_path(build_id);
    if ensure_parent(&path).is_ok() {
        if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(file, "{} [{}] {}", Local::now().to_rfc3339(), level, message);
        }
    }
    let _ = app.emit("build-log", LogEvent {
        build_id: build_id.to_string(),
        level: level.to_string(),
        message: message.to_string(),
    });
}
