use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BuildTarget {
    #[default]
    Auto,
    Android,
    Tauri,
    Electron,
    Dotnet,
}

impl BuildTarget {
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Automatic",
            Self::Android => "Android APK / AAB",
            Self::Tauri => "Tauri Windows EXE / MSI",
            Self::Electron => "Electron Windows EXE",
            Self::Dotnet => ".NET Windows EXE",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StepDisposition {
    Run,
    Setup,
    Collect,
    Skip,
    Block,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedStep {
    pub name: String,
    pub disposition: StepDisposition,
    pub command: Option<String>,
    pub shell: Option<String>,
    pub working_directory: Option<String>,
    pub reason: String,
    #[serde(default)]
    pub env: BTreeMap<String, Value>,
    pub continue_on_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolRequirement {
    pub id: String,
    pub label: String,
    pub version: Option<String>,
    pub portable: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactRule {
    pub pattern: String,
    pub kind: String,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildPlan {
    pub target: BuildTarget,
    pub target_label: String,
    pub supported: bool,
    pub summary: String,
    pub steps: Vec<PlannedStep>,
    pub tools: Vec<ToolRequirement>,
    pub required_secrets: Vec<String>,
    pub artifacts: Vec<ArtifactRule>,
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkflowDoc {
    pub name: Option<String>,
    #[serde(default)]
    pub on: Value,
    #[serde(default)]
    pub env: BTreeMap<String, Value>,
    pub jobs: BTreeMap<String, JobDoc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct JobDoc {
    pub name: Option<String>,
    #[serde(rename = "runs-on", default)]
    pub runs_on: Value,
    #[serde(default)]
    pub env: BTreeMap<String, Value>,
    #[serde(default)]
    pub steps: Vec<StepDoc>,
    #[serde(default)]
    pub strategy: Value,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct StepDoc {
    pub name: Option<String>,
    pub uses: Option<String>,
    pub run: Option<String>,
    #[serde(default)]
    pub shell: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, Value>,
    #[serde(default)]
    pub with: BTreeMap<String, Value>,
    #[serde(default)]
    pub working_directory: Option<String>,
    #[serde(default)]
    pub r#if: Option<String>,
    #[serde(default)]
    pub continue_on_error: bool,
}

pub fn load_workflow(path: &Path) -> Result<WorkflowDoc, String> {
    let text = fs::read_to_string(path).map_err(|err| err.to_string())?;
    serde_yaml::from_str(&text).map_err(|err| format!("{}: {}", path.display(), err))
}

pub fn detect_target(project: &Path) -> Option<BuildTarget> {
    if project.join("src-tauri").join("Cargo.toml").exists() && project.join("package.json").exists() {
        return Some(BuildTarget::Tauri);
    }
    if package_mentions(project, &["electron", "electron-builder", "electron-forge"]) {
        return Some(BuildTarget::Electron);
    }
    if project.join("gradlew").exists() || project.join("gradlew.bat").exists() {
        return Some(BuildTarget::Android);
    }
    if contains_extension(project, &["sln", "csproj"], 2) {
        return Some(BuildTarget::Dotnet);
    }
    None
}

pub fn analyze(
    project: &Path,
    workflow_path: Option<&Path>,
    job_id: Option<&str>,
    requested_target: BuildTarget,
) -> Result<BuildPlan, String> {
    let target = match requested_target {
        BuildTarget::Auto => detect_target(project).ok_or_else(|| {
            "Could not identify an Android, Tauri, Electron, or .NET project. Choose a target explicitly or add a supported project file.".to_string()
        })?,
        target => target,
    };

    let mut plan = BuildPlan {
        target,
        target_label: target.label().to_string(),
        supported: true,
        summary: format!("{} local build", target.label()),
        steps: vec![],
        tools: default_tools(target),
        required_secrets: vec![],
        artifacts: default_artifacts(target),
        blockers: vec![],
        warnings: vec![],
        env: BTreeMap::new(),
    };

    if let Some(path) = workflow_path.filter(|path| !path.as_os_str().is_empty()) {
        let doc = load_workflow(path)?;
        let selected_id = job_id.filter(|value| !value.trim().is_empty()).or_else(|| doc.jobs.keys().next().map(String::as_str));
        let selected_id = selected_id.ok_or_else(|| "The workflow has no jobs.".to_string())?;
        let job = doc.jobs.get(selected_id).ok_or_else(|| format!("Job '{}' was not found.", selected_id))?;
        analyze_job(&doc, job, &mut plan);
    } else {
        plan.steps = default_steps(project, target)?;
    }

    dedupe_tools(&mut plan.tools);
    plan.required_secrets.sort();
    plan.required_secrets.dedup();
    dedupe_artifacts(&mut plan.artifacts);
    plan.supported = plan.blockers.is_empty() && plan.steps.iter().all(|step| step.disposition != StepDisposition::Block);
    if !plan.supported {
        plan.summary = format!("{} has {} blocker(s)", plan.target_label, plan.blockers.len());
    }
    Ok(plan)
}

fn analyze_job(doc: &WorkflowDoc, job: &JobDoc, plan: &mut BuildPlan) {
    plan.env = doc.env.clone();
    plan.env.extend(job.env.clone());
    let runner = value_to_string(&job.runs_on).unwrap_or_default().to_ascii_lowercase();
    if runner.contains("ubuntu") || runner.contains("macos") {
        plan.blockers.push(format!(
            "This job targets '{}'. Select a Windows job or use a native project build plan.",
            runner
        ));
    }
    if !matches!(job.strategy, Value::Null) {
        plan.blockers.push("Matrix strategies need an explicit locally selected matrix value and are not run implicitly.".to_string());
    }

    let mut required = HashSet::new();
    for value in doc.env.values().chain(job.env.values()) {
        if let Some(text) = value_to_string(value) {
            required.extend(required_secrets_in_text(&text));
        }
    }

    for (index, step) in job.steps.iter().enumerate() {
        let name = step.name.clone().unwrap_or_else(|| format!("Step {}", index + 1));
        for value in step.env.values().chain(step.with.values()) {
            if let Some(text) = value_to_string(value) {
                required.extend(required_secrets_in_text(&text));
            }
        }
        if let Some(run) = &step.run {
            required.extend(required_secrets_in_text(run));
        }

        let mut planned = PlannedStep {
            name: name.clone(),
            disposition: StepDisposition::Run,
            command: step.run.clone(),
            shell: step.shell.clone(),
            working_directory: step.working_directory.clone(),
            reason: "Runs locally in the isolated build workspace.".to_string(),
            env: step.env.clone(),
            continue_on_error: step.continue_on_error,
        };

        if let Some(condition) = &step.r#if {
            if !is_supported_condition(condition) {
                planned.disposition = StepDisposition::Block;
                planned.reason = format!("Condition '{}' cannot be evaluated safely offline.", condition);
                plan.blockers.push(format!("{}: {}", name, planned.reason));
            }
        }

        if let Some(uses) = &step.uses {
            classify_action(uses, step, &mut planned, plan);
        } else if step.run.is_none() {
            planned.disposition = StepDisposition::Skip;
            planned.reason = "Step has no action or command.".to_string();
        } else if is_side_effecting_step(&name, step.run.as_deref().unwrap_or_default()) {
            planned.disposition = StepDisposition::Skip;
            planned.reason = "Skipped by the local build-only safety policy (release, publish, deploy, or upload side effect).".to_string();
            plan.warnings.push(format!("{} will be skipped by the build-only safety policy.", name));
        }

        if step.continue_on_error {
            plan.warnings.push(format!("{} is marked continue-on-error; a failure will be logged and the build will continue.", name));
        }
        plan.steps.push(planned);
    }
    plan.required_secrets = required.into_iter().collect();
}

fn classify_action(uses: &str, step: &StepDoc, planned: &mut PlannedStep, plan: &mut BuildPlan) {
    let lower = uses.to_ascii_lowercase();
    if lower.starts_with("actions/checkout@") || lower.starts_with("actions/cache@") {
        planned.disposition = StepDisposition::Skip;
        planned.reason = if lower.starts_with("actions/checkout@") {
            "Satisfied by the isolated local workspace.".to_string()
        } else {
            "Hosted cache action is replaced by persistent local tool caches.".to_string()
        };
        return;
    }
    if lower.starts_with("actions/setup-java@") {
        planned.disposition = StepDisposition::Setup;
        planned.reason = "Java is resolved into the per-user tool cache.".to_string();
        let version = step.with.get("java-version").and_then(value_to_string);
        plan.tools.push(tool("java", "Java JDK", version, "Required by the selected workflow."));
        return;
    }
    if lower.starts_with("actions/setup-node@") {
        planned.disposition = StepDisposition::Setup;
        planned.reason = "Node.js is resolved into the per-user tool cache.".to_string();
        let version = step.with.get("node-version").and_then(value_to_string);
        plan.tools.push(tool("node", "Node.js", version, "Required by the selected workflow."));
        return;
    }
    if lower.contains("rust-toolchain") || lower.starts_with("actions-rs/toolchain@") {
        planned.disposition = StepDisposition::Setup;
        planned.reason = "Rust and a portable GNU linker are resolved without Visual Studio.".to_string();
        plan.tools.push(tool("rust", "Rust + portable GCC", Some("stable".to_string()), "Required by the selected workflow."));
        return;
    }
    if lower.starts_with("actions/setup-dotnet@") {
        planned.disposition = StepDisposition::Setup;
        planned.reason = ".NET SDK is resolved into the per-user tool cache.".to_string();
        let version = step.with.get("dotnet-version").and_then(value_to_string);
        plan.tools.push(tool("dotnet", ".NET SDK", version, "Required by the selected workflow."));
        return;
    }
    if lower.starts_with("actions/upload-artifact@") {
        planned.disposition = StepDisposition::Collect;
        planned.reason = "Files are collected into the local build output instead of uploaded.".to_string();
        if let Some(paths) = step.with.get("path").and_then(value_to_string) {
            for pattern in artifact_lines(&paths) {
                plan.artifacts.push(ArtifactRule {
                    kind: artifact_kind(&pattern),
                    pattern,
                    required: true,
                });
            }
        } else {
            planned.disposition = StepDisposition::Block;
            planned.reason = "upload-artifact has no path.".to_string();
            plan.blockers.push(format!("{}: upload-artifact has no path.", planned.name));
        }
        return;
    }
    if is_publish_action(&lower) {
        planned.disposition = StepDisposition::Skip;
        planned.reason = "Skipped by the local build-only safety policy.".to_string();
        plan.warnings.push(format!("{} ({}) will not publish or deploy from a local build.", planned.name, uses));
        return;
    }
    planned.disposition = StepDisposition::Block;
    planned.reason = format!("Action '{}' has no safe local adapter.", uses);
    plan.blockers.push(format!("{}: {}", planned.name, planned.reason));
}

fn default_steps(project: &Path, target: BuildTarget) -> Result<Vec<PlannedStep>, String> {
    let steps = match target {
        BuildTarget::Android => vec![run_step("Build release APK and AAB", ".\\gradlew.bat assembleRelease bundleRelease")],
        BuildTarget::Tauri => vec![
            run_step("Install locked frontend dependencies", "npm.cmd ci"),
            run_step("Build Windows installers", "npm.cmd run tauri build"),
        ],
        BuildTarget::Electron => {
            let script = preferred_package_script(project, &["dist", "package", "make", "build"])
                .ok_or_else(|| "Electron project has no dist, package, make, or build script.".to_string())?;
            vec![
                run_step("Install locked frontend dependencies", "npm.cmd ci"),
                run_step("Build Windows package", &format!("npm.cmd run {}", script)),
            ]
        }
        BuildTarget::Dotnet => vec![run_step("Publish self-contained Windows application", "dotnet.exe publish -c Release -r win-x64 --self-contained true")],
        BuildTarget::Auto => unreachable!(),
    };
    Ok(steps)
}

fn default_tools(target: BuildTarget) -> Vec<ToolRequirement> {
    let mut tools = vec![tool("git", "Git", None, "Provides source preparation and optional Git Bash.")];
    match target {
        BuildTarget::Android => {
            tools.push(tool("java", "Java JDK", Some("17".to_string()), "Required by Gradle and Android tooling."));
            tools.push(tool("android", "Android SDK", None, "Required to compile and optionally install Android artifacts."));
        }
        BuildTarget::Tauri => {
            tools.push(tool("node", "Node.js", Some("22".to_string()), "Builds the frontend and runs the Tauri CLI."));
            tools.push(tool("rust", "Rust + portable GCC", Some("stable".to_string()), "Builds Windows binaries without Visual Studio or admin rights."));
        }
        BuildTarget::Electron => {
            tools.push(tool("node", "Node.js", Some("22".to_string()), "Builds and packages the Electron application."));
        }
        BuildTarget::Dotnet => {
            tools.push(tool("dotnet", ".NET SDK", Some("8.0".to_string()), "Publishes a self-contained Windows executable."));
        }
        BuildTarget::Auto => {}
    }
    tools
}

fn default_artifacts(target: BuildTarget) -> Vec<ArtifactRule> {
    let patterns: &[(&str, &str)] = match target {
        BuildTarget::Android => &[("**/*.apk", "apk"), ("**/*.aab", "aab")],
        BuildTarget::Tauri => &[
            ("src-tauri/target/**/release/bundle/nsis/*.exe", "exe"),
            ("src-tauri/target/**/release/bundle/msi/*.msi", "msi"),
            ("src-tauri/target/release/bundle/nsis/*.exe", "exe"),
            ("src-tauri/target/release/bundle/msi/*.msi", "msi"),
        ],
        BuildTarget::Electron => &[("dist/**/*.exe", "exe"), ("release/**/*.exe", "exe"), ("out/**/*.exe", "exe")],
        BuildTarget::Dotnet => &[("**/bin/Release/**/publish/*.exe", "exe")],
        BuildTarget::Auto => &[],
    };
    patterns
        .iter()
        .map(|(pattern, kind)| ArtifactRule { pattern: (*pattern).to_string(), kind: (*kind).to_string(), required: true })
        .collect()
}

fn run_step(name: &str, command: &str) -> PlannedStep {
    PlannedStep {
        name: name.to_string(),
        disposition: StepDisposition::Run,
        command: Some(command.to_string()),
        shell: Some("powershell".to_string()),
        working_directory: None,
        reason: "Generated by the native project adapter.".to_string(),
        env: BTreeMap::new(),
        continue_on_error: false,
    }
}

fn tool(id: &str, label: &str, version: Option<String>, reason: &str) -> ToolRequirement {
    ToolRequirement {
        id: id.to_string(),
        label: label.to_string(),
        version,
        portable: true,
        reason: reason.to_string(),
    }
}

fn package_mentions(project: &Path, needles: &[&str]) -> bool {
    fs::read_to_string(project.join("package.json"))
        .map(|text| needles.iter().any(|needle| text.to_ascii_lowercase().contains(needle)))
        .unwrap_or(false)
}

fn preferred_package_script(project: &Path, names: &[&str]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(&fs::read_to_string(project.join("package.json")).ok()?).ok()?;
    let scripts = value.get("scripts")?.as_object()?;
    names.iter().find(|name| scripts.contains_key(**name)).map(|name| (*name).to_string())
}

fn contains_extension(root: &Path, extensions: &[&str], depth: usize) -> bool {
    if depth == 0 {
        return false;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        if path.is_dir() {
            contains_extension(&path, extensions, depth - 1)
        } else {
            path.extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| extensions.iter().any(|expected| ext.eq_ignore_ascii_case(expected)))
                .unwrap_or(false)
        }
    })
}

fn required_secrets_in_text(text: &str) -> HashSet<String> {
    let mut names = HashSet::new();
    for marker in ["${{ secrets.", "${{secrets."] {
        let mut rest = text;
        while let Some(start) = rest.find(marker) {
            let after = &rest[start + marker.len()..];
            if let Some(end) = after.find("}}") {
                let name = after[..end].trim();
                if !name.is_empty() {
                    names.insert(name.to_string());
                }
                rest = &after[end + 2..];
            } else {
                break;
            }
        }
    }
    names
}

fn artifact_lines(value: &str) -> Vec<String> {
    value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('!'))
        .map(str::to_string)
        .collect()
}

fn artifact_kind(pattern: &str) -> String {
    PathBuf::from(pattern)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("file")
        .trim_start_matches('*')
        .to_ascii_lowercase()
}

fn is_side_effecting_step(name: &str, command: &str) -> bool {
    let text = format!("{} {}", name, command).to_ascii_lowercase();
    ["publish", "release", "deploy", "gh release", "npm publish", "dotnet nuget push", "curl -x post", "invoke-restmethod"]
        .iter()
        .any(|needle| text.contains(needle))
}

fn is_publish_action(action: &str) -> bool {
    ["release", "deploy", "publish", "pages", "app-store", "play-store", "winget"]
        .iter()
        .any(|needle| action.contains(needle))
}

fn is_supported_condition(condition: &str) -> bool {
    matches!(
        condition.trim().to_ascii_lowercase().as_str(),
        "success()" | "always()" | "${{ success() }}" | "${{ always() }}" | "true" | "${{ true }}"
    )
}

fn dedupe_tools(tools: &mut Vec<ToolRequirement>) {
    let mut seen = HashSet::new();
    tools.reverse();
    tools.retain(|tool| seen.insert(tool.id.clone()));
    tools.reverse();
}

fn dedupe_artifacts(artifacts: &mut Vec<ArtifactRule>) {
    let mut seen = HashSet::new();
    artifacts.retain(|artifact| seen.insert(artifact.pattern.clone()));
}

pub fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_project(name: &str) -> PathBuf {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("abl-planning-{}-{}", name, nonce));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn repository_root() -> PathBuf {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        loop {
            if path.join("package.json").exists() && path.join(".github").exists() {
                return path;
            }
            assert!(path.pop(), "could not locate repository root");
        }
    }

    #[test]
    fn own_workflow_is_supported_as_a_tauri_windows_plan() {
        let project = repository_root();
        let workflow = project.join(".github").join("workflows").join("build-main.yml");
        let plan = analyze(&project, Some(&workflow), Some("build-windows"), BuildTarget::Auto).unwrap();

        assert_eq!(plan.target, BuildTarget::Tauri);
        assert!(plan.supported, "{:?}", plan.blockers);
        assert!(plan.tools.iter().any(|tool| tool.id == "node"));
        assert!(plan.tools.iter().any(|tool| tool.id == "rust"));
        assert!(plan.artifacts.iter().any(|artifact| artifact.kind == "exe"));
        assert!(plan.artifacts.iter().any(|artifact| artifact.kind == "msi"));
    }

    #[test]
    fn dynamic_secrets_and_multiline_artifacts_are_exposed_before_build() {
        let project = temp_project("workflow");
        fs::write(project.join("gradlew.bat"), "").unwrap();
        let workflow = project.join("build.yml");
        fs::write(
            &workflow,
            r#"
name: Build
on: push
env:
  STORE_FILE: ${{ secrets.STORE_FILE }}
jobs:
  apk:
    runs-on: windows-latest
    steps:
      - run: .\gradlew.bat assembleRelease
        env:
          STORE_PASSWORD: ${{secrets.STORE_PASSWORD}}
      - uses: actions/upload-artifact@v4
        with:
          path: |
            app/build/**/*.apk
            app/build/**/*.aab
            !app/build/**/unsigned*
"#,
        )
        .unwrap();

        let plan = analyze(&project, Some(&workflow), Some("apk"), BuildTarget::Auto).unwrap();
        assert_eq!(plan.required_secrets, vec!["STORE_FILE", "STORE_PASSWORD"]);
        assert!(plan.artifacts.iter().any(|artifact| artifact.pattern == "app/build/**/*.apk"));
        assert!(!plan.artifacts.iter().any(|artifact| artifact.pattern.starts_with('!')));
        fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn deployment_actions_are_skipped_but_unknown_build_actions_block() {
        let project = temp_project("safety");
        fs::write(project.join("gradlew.bat"), "").unwrap();
        let workflow = project.join("build.yml");
        fs::write(
            &workflow,
            r#"
jobs:
  apk:
    runs-on: windows-latest
    steps:
      - uses: vendor/custom-build@v1
      - uses: vendor/deploy-release@v1
"#,
        )
        .unwrap();

        let plan = analyze(&project, Some(&workflow), Some("apk"), BuildTarget::Android).unwrap();
        assert!(!plan.supported);
        assert_eq!(plan.steps[0].disposition, StepDisposition::Block);
        assert_eq!(plan.steps[1].disposition, StepDisposition::Skip);
        fs::remove_dir_all(project).unwrap();
    }
}
