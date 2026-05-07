use base64::Engine;
use chrono::Local;
use glob::glob;
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
};
use tauri::{AppHandle, Emitter, Manager, State};

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

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BuildRequest {
    repo_url: String,
    ref_name: String,
    output_folder: String,
    workflow_path: String,
    job_id: String,
    shell_mode: ShellMode,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BuildResult {
    build_id: String,
    output_folder: String,
    apk_files: Vec<String>,
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
    jobs: BTreeMap<String, JobDoc>,
}

#[derive(Debug, Clone, Deserialize)]
struct JobDoc {
    name: Option<String>,
    #[serde(rename = "runs-on", default)]
    runs_on: Value,
    #[serde(default)]
    env: BTreeMap<String, Value>,
    #[serde(default)]
    steps: Vec<StepDoc>,
}

#[derive(Debug, Clone, Deserialize)]
struct StepDoc {
    name: Option<String>,
    uses: Option<String>,
    run: Option<String>,
    #[serde(default)]
    shell: Option<String>,
    #[serde(default)]
    env: BTreeMap<String, Value>,
    #[serde(default)]
    with: BTreeMap<String, Value>,
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(BuildState::default())
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            prepare_repo,
            detect_workflows,
            get_secrets,
            save_secrets,
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
fn prepare_repo(repo_url: String, ref_name: String) -> Result<String, String> {
    validate_tools(false)?;
    let repo_path = repo_path_for(&repo_url)?;
    if repo_path.exists() {
        run_checked(
            CommandSpec::new("git", ["fetch", "--all", "--prune"]).cwd(&repo_path),
            None,
        )?;
    } else {
        ensure_dir(&default_repo_root())?;
        let repo_path_text = repo_path.to_string_lossy().to_string();
        run_checked(CommandSpec::new("git", ["clone", repo_url.as_str(), repo_path_text.as_str()]), None)?;
    }

    checkout_ref(&repo_path, &ref_name)?;
    protect_local_properties(&repo_path)?;
    Ok(repo_path.display().to_string())
}

#[tauri::command]
fn detect_workflows(repo_path: String) -> Result<Vec<WorkflowSummary>, String> {
    let workflow_dir = PathBuf::from(repo_path).join(".github").join("workflows");
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
            file_path: path.display().to_string(),
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
    log(&app, &build_id, "group", "Preparing repository");
    validate_tools(matches!(request.shell_mode, ShellMode::Bash))?;
    let repo_path = PathBuf::from(prepare_repo(request.repo_url.clone(), request.ref_name.clone())?);
    log(&app, &build_id, "success", &format!("Repository ready at {}", repo_path.display()));

    check_android_sdk()?;
    let doc = load_workflow(&request.workflow_path)?;
    let job = doc.jobs.get(&request.job_id).ok_or_else(|| format!("Job '{}' was not found", request.job_id))?.clone();
    let secrets = unprotect_secret_store(&stable_id(&request.repo_url))?;
    let mut missing = required_secrets(&doc, &job)
        .into_iter()
        .filter(|name| !secrets.contains_key(name))
        .collect::<Vec<_>>();
    missing.sort();
    if !missing.is_empty() {
        return Err(format!("Missing local secrets: {}. Add them on the Secrets screen.", missing.join(", ")));
    }

    let context = Context {
        workspace: repo_path.clone(),
        ref_name: request.ref_name.clone(),
        workflow_env: value_map_to_strings(&doc.env),
        job_env: value_map_to_strings(&job.env),
        secrets,
    };

    for (index, step) in job.steps.iter().enumerate() {
        if cancel.load(Ordering::SeqCst) {
            return Err("Build cancelled".to_string());
        }
        let step_name = step.name.clone().unwrap_or_else(|| format!("Step {}", index + 1));
        log(&app, &build_id, "group", &format!("Step {}: {}", index + 1, step_name));
        run_step(&app, &build_id, &request, &repo_path, &context, step, cancel.clone())?;
        log(&app, &build_id, "success", &format!("Finished {}", step_name));
    }

    let final_output = output_folder(&request.output_folder, &request.repo_url, &request.ref_name)?;
    let copied = copy_apks(&repo_path, &final_output)?;
    if copied.is_empty() {
        return Err("Build finished, but no APK files were found to copy".to_string());
    }
    sync_latest(&final_output, &request.output_folder, &request.repo_url, &copied)?;
    log(&app, &build_id, "success", &format!("Copied {} APK file(s) to {}", copied.len(), final_output.display()));
    Ok(BuildResult {
        build_id,
        output_folder: final_output.display().to_string(),
        apk_files: copied.into_iter().map(|path| path.display().to_string()).collect(),
    })
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
            validate_java_version(&version)?;
            log(app, build_id, "success", &format!("Java {} is available", version));
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
    command.args(args).cwd(cwd).stdout(Stdio::piped()).stderr(Stdio::piped());
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
    workflow_env: HashMap<String, String>,
    job_env: HashMap<String, String>,
    secrets: HashMap<String, String>,
}

impl Context {
    fn env_for_step(&self, step: &StepDoc) -> Result<HashMap<String, String>, String> {
        let mut env = self.workflow_env.clone();
        env.extend(self.job_env.clone());
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

fn load_workflow(path: &str) -> Result<WorkflowDoc, String> {
    let text = fs::read_to_string(path).map_err(display_err)?;
    serde_yaml::from_str(&text).map_err(display_err)
}

fn checkout_ref(repo_path: &Path, ref_name: &str) -> Result<(), String> {
    let trimmed = ref_name.trim();
    if trimmed.chars().all(|c| c.is_ascii_digit()) {
        let pr_ref = format!("pull/{}/head:pr-{}", trimmed, trimmed);
        run_checked(CommandSpec::new("git", ["fetch", "origin", &pr_ref]).cwd(repo_path), None)?;
        run_checked(CommandSpec::new("git", ["checkout", &format!("pr-{}", trimmed)]).cwd(repo_path), None)
    } else {
        let remote_ref = format!("origin/{}", trimmed);
        let checkout_target = if run_checked(CommandSpec::new("git", ["rev-parse", "--verify", &remote_ref]).cwd(repo_path), None).is_ok() {
            remote_ref
        } else {
            trimmed.to_string()
        };
        run_checked(CommandSpec::new("git", ["checkout", &checkout_target]).cwd(repo_path), None)
    }
}

fn validate_tools(needs_bash: bool) -> Result<(), String> {
    run_checked(CommandSpec::new("git", ["--version"]), None).map_err(|_| "Git is required. Install Git for Windows and ensure git is in PATH.".to_string())?;
    validate_java_version("17")?;
    if needs_bash && find_git_bash().is_none() {
        return Err("Git Bash is required for Bash compatibility mode. Install Git for Windows.".to_string());
    }
    Ok(())
}

fn validate_java_version(expected: &str) -> Result<(), String> {
    let output = Command::new("java").arg("-version").output().map_err(|_| "Java 17 is required. Install Temurin/OpenJDK 17 and ensure java is in PATH.".to_string())?;
    let version_text = String::from_utf8_lossy(&output.stderr).to_string() + &String::from_utf8_lossy(&output.stdout);
    if expected == "17" && !version_text.contains("\"17.") && !version_text.contains(" version \"17") {
        return Err("Java 17 is required, but the detected java version is different.".to_string());
    }
    Ok(())
}

fn check_android_sdk() -> Result<(), String> {
    if std::env::var("ANDROID_HOME").is_ok() || std::env::var("ANDROID_SDK_ROOT").is_ok() {
        return Ok(());
    }
    let fallback = dirs::data_local_dir().unwrap_or_else(default_app_data).join("Android").join("Sdk");
    if fallback.exists() {
        return Ok(());
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
    use windows_sys::Win32::Security::Cryptography::{CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, DATA_BLOB};
    let mut input = DATA_BLOB { cbData: value.as_bytes().len() as u32, pbData: value.as_ptr() as *mut u8 };
    let mut output = DATA_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    let ok = unsafe { CryptProtectData(&mut input, std::ptr::null(), std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut(), CRYPTPROTECT_UI_FORBIDDEN, &mut output) };
    if ok == 0 {
        return Err("Windows DPAPI failed while saving a secret".to_string());
    }
    let bytes = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) };
    Ok(format!("dpapi:{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

#[cfg(windows)]
fn unprotect_secret(value: &str) -> Result<String, String> {
    use windows_sys::Win32::Security::Cryptography::{CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, DATA_BLOB};
    let encoded = value.strip_prefix("dpapi:").unwrap_or(value);
    let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).map_err(display_err)?;
    let mut input = DATA_BLOB { cbData: bytes.len() as u32, pbData: bytes.as_ptr() as *mut u8 };
    let mut output = DATA_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    let ok = unsafe { CryptUnprotectData(&mut input, std::ptr::null_mut(), std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut(), CRYPTPROTECT_UI_FORBIDDEN, &mut output) };
    if ok == 0 {
        return Err("Windows DPAPI failed while reading a secret".to_string());
    }
    let bytes = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) };
    String::from_utf8(bytes.to_vec()).map_err(display_err)
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

fn value_map_to_strings(map: &BTreeMap<String, Value>) -> HashMap<String, String> {
    map.iter().filter_map(|(key, value)| value_to_string(value).map(|text| (key.clone(), text))).collect()
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
    [
        r"C:\Program Files\Git\bin\bash.exe",
        r"C:\Program Files (x86)\Git\bin\bash.exe",
    ].into_iter().map(PathBuf::from).find(|path| path.exists())
}

fn repo_path_for(repo_url: &str) -> Result<PathBuf, String> {
    Ok(default_repo_root().join(format!("{}-{}", sanitize(&repo_name(repo_url)), &stable_id(repo_url)[..8])))
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

fn log(app: &AppHandle, build_id: &str, level: &str, message: &str) {
    let _ = app.emit("build-log", LogEvent {
        build_id: build_id.to_string(),
        level: level.to_string(),
        message: message.to_string(),
    });
}
