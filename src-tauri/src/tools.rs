use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use crate::planning::ToolRequirement;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    io::{self, Cursor, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Mutex, OnceLock},
    time::Duration,
};
use zip::ZipArchive;

const ANDROID_CMDLINE_TOOLS_URL: &str = "https://dl.google.com/android/repository/commandlinetools-win-14742923_latest.zip";
const LICENSE_SENTINEL: &str = ".apk-build-launcher-licenses-ok";

static GIT_CACHE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
static GIT_BASH_CACHE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
static JAVA_CACHE: OnceLock<Mutex<HashMap<String, JavaInstall>>> = OnceLock::new();
static ANDROID_SDK_CACHE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub tools_root: String,
    pub git: ToolProbe,
    pub java: ToolProbe,
    pub android_sdk: ToolProbe,
    pub git_bash: ToolProbe,
    pub node: ToolProbe,
    pub rust: ToolProbe,
    pub dotnet: ToolProbe,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolProbe {
    pub available: bool,
    pub path: Option<String>,
    pub message: String,
}

#[derive(Clone)]
pub struct JavaInstall {
    pub java: PathBuf,
    pub home: Option<PathBuf>,
}

pub struct BuildTools {
    pub java: JavaInstall,
    pub android_sdk: PathBuf,
}

#[derive(Clone)]
pub struct PlanTools {
    pub env: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    digest: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NodeRelease {
    version: String,
    lts: serde_json::Value,
}

pub fn tool_status() -> ToolStatus {
    let tools_root = tools_root();
    let git = git_program()
        .filter(|path| run_output(path, &["--version"], None).is_ok())
        .map_or_else(
            || ToolProbe {
                available: false,
                path: None,
                message: "Git will be downloaded when needed.".to_string(),
            },
            |path| ToolProbe {
                available: true,
                path: Some(path.display().to_string()),
                message: "Git is available.".to_string(),
            },
        );
    let java = find_java("17").map_or_else(
        |_| ToolProbe {
            available: false,
            path: None,
            message: "JDK 17 will be downloaded when needed.".to_string(),
        },
        |install| ToolProbe {
            available: true,
            path: Some(install.java.display().to_string()),
            message: "Java 17 is available.".to_string(),
        },
    );
    let android_sdk = android_sdk_path().map_or_else(
        |_| ToolProbe {
            available: false,
            path: None,
            message: "Android SDK will be downloaded when needed.".to_string(),
        },
        |path| ToolProbe {
            available: true,
            path: Some(path.display().to_string()),
            message: "Android SDK is available.".to_string(),
        },
    );
    let git_bash = find_git_bash().map_or_else(
        || ToolProbe {
            available: false,
            path: None,
            message: "Git Bash is optional unless Bash mode is selected.".to_string(),
        },
        |path| ToolProbe {
            available: true,
            path: Some(path.display().to_string()),
            message: "Git Bash is available.".to_string(),
        },
    );
    let node = probe_command(
        node_program(),
        &["--version"],
        "Node.js will be downloaded when required.",
        "Node.js is available.",
    );
    let rust = probe_command(
        cargo_program(),
        &["--version"],
        "Rust and portable GCC will be downloaded when required.",
        "Rust is available.",
    );
    let dotnet = probe_command(
        dotnet_program(),
        &["--version"],
        ".NET SDK will be downloaded when required.",
        ".NET SDK is available.",
    );

    ToolStatus {
        tools_root: tools_root.display().to_string(),
        git,
        java,
        android_sdk,
        git_bash,
        node,
        rust,
        dotnet,
    }
}

pub fn ensure_git<L>(mut log: L) -> Result<PathBuf, String>
where
    L: FnMut(&str, &str),
{
    if let Some(path) = cached_git() {
        log("success", &format!("Git is available at {}", path.display()));
        return Ok(path);
    }

    if let Some(path) = git_program().filter(|path| run_output(path, &["--version"], None).is_ok()) {
        set_cached_git(path.clone());
        log("success", &format!("Git is available at {}", path.display()));
        return Ok(path);
    }

    let destination = tools_root().join("Git");
    let installed = destination.join("cmd").join("git.exe");
    if installed.exists() && run_output(&installed, &["--version"], None).is_ok() {
        set_cached_git(installed.clone());
        log("success", &format!("Git is available at {}", installed.display()));
        return Ok(installed);
    }

    log("group", "Downloading portable Git");
    let url = latest_mingit_url()?;
    let archive = download_bytes(&url, &mut log)?;
    ensure_clean_dir(&destination)?;
    extract_zip_bytes(&archive, &destination)?;
    if installed.exists() {
        set_cached_git(installed.clone());
        log("success", &format!("Portable Git installed at {}", destination.display()));
        Ok(installed)
    } else {
        Err("Portable Git was downloaded, but cmd\\git.exe was not found in the archive.".to_string())
    }
}

pub fn ensure_java_version<L>(expected: &str, mut log: L) -> Result<JavaInstall, String>
where
    L: FnMut(&str, &str),
{
    let major = expected_major_text(expected);
    if let Some(java) = cached_java(&major) {
        log("success", &format!("Java {} is available at {}", major, java.java.display()));
        return Ok(java);
    }

    if let Ok(java) = find_java(expected) {
        set_cached_java(major.clone(), java.clone());
        log("success", &format!("Java {} is available at {}", major, java.java.display()));
        return Ok(java);
    }

    let destination = tools_root().join(format!("jdk-{}", major));
    log("group", &format!("Downloading Temurin JDK {}", major));
    let url = format!(
        "https://api.adoptium.net/v3/binary/latest/{}/ga/windows/x64/jdk/hotspot/normal/eclipse?project=jdk",
        major
    );
    let archive = download_bytes(&url, &mut log)?;
    let temp = tools_root().join(format!("_jdk-{}-extract", major));
    ensure_clean_dir(&temp)?;
    extract_zip_bytes(&archive, &temp)?;
    let home = single_child_dir(&temp).unwrap_or(temp.clone());
    ensure_clean_dir(&destination)?;
    copy_dir_all(&home, &destination).map_err(display_err)?;
    let _ = fs::remove_dir_all(&temp);
    let java = find_java(expected).map_err(|_| format!("JDK {} was downloaded, but java.exe was not found.", major))?;
    set_cached_java(major, java.clone());
    Ok(java)
}

pub fn ensure_build_tools<L>(
    java_version: &str,
    compile_sdks: &[u32],
    needs_bash: bool,
    mut log: L,
) -> Result<BuildTools, String>
where
    L: FnMut(&str, &str),
{
    let java = ensure_java_version(java_version, |level, message| log(level, message))?;
    if needs_bash {
        ensure_git_bash(|level, message| log(level, message))?;
    }
    let android_sdk = ensure_android_sdk(compile_sdks, &java, |level, message| log(level, message))?;
    Ok(BuildTools { java, android_sdk })
}

pub fn ensure_plan_tools<L>(
    requirements: &[ToolRequirement],
    compile_sdks: &[u32],
    mut log: L,
) -> Result<PlanTools, String>
where
    L: FnMut(&str, &str),
{
    let mut env = HashMap::new();
    let mut path_prefixes = vec![];
    for requirement in requirements {
        match requirement.id.as_str() {
            "git" => {
                let git = ensure_git(|level, message| log(level, message))?;
                if let Some(parent) = git.parent() {
                    path_prefixes.push(parent.to_path_buf());
                }
            }
            "java" => {
                let version = requirement.version.as_deref().unwrap_or("17");
                let java = ensure_java_version(version, |level, message| log(level, message))?;
                if let Some(home) = java.home {
                    env.insert("JAVA_HOME".to_string(), home.display().to_string());
                    path_prefixes.push(home.join("bin"));
                }
            }
            "android" => {
                let java = ensure_java_version("17", |level, message| log(level, message))?;
                let sdk = ensure_android_sdk(compile_sdks, &java, |level, message| log(level, message))?;
                env.insert("ANDROID_HOME".to_string(), sdk.display().to_string());
                env.insert("ANDROID_SDK_ROOT".to_string(), sdk.display().to_string());
            }
            "node" => {
                let node = ensure_node(requirement.version.as_deref(), |level, message| log(level, message))?;
                if let Some(parent) = node.parent() {
                    path_prefixes.push(parent.to_path_buf());
                }
            }
            "rust" => {
                let rust = ensure_rust_gnu(|level, message| log(level, message))?;
                path_prefixes.push(rust.cargo_bin.clone());
                path_prefixes.push(rust.linker_bin.clone());
                env.insert("CARGO_HOME".to_string(), rust.cargo_home.display().to_string());
                env.insert("RUSTUP_HOME".to_string(), rust.rustup_home.display().to_string());
                env.insert("RUSTUP_TOOLCHAIN".to_string(), "stable-x86_64-pc-windows-gnu".to_string());
            }
            "dotnet" => {
                let dotnet = ensure_dotnet(requirement.version.as_deref(), |level, message| log(level, message))?;
                if let Some(parent) = dotnet.parent() {
                    path_prefixes.push(parent.to_path_buf());
                    env.insert("DOTNET_ROOT".to_string(), parent.display().to_string());
                }
            }
            other => return Err(format!("Unknown local tool requirement '{}'.", other)),
        }
    }
    let separator = if cfg!(windows) { ";" } else { ":" };
    let existing = std::env::var("PATH").unwrap_or_default();
    let mut path = path_prefixes
        .iter()
        .map(|item| item.display().to_string())
        .collect::<Vec<_>>()
        .join(separator);
    if !path.is_empty() && !existing.is_empty() {
        path.push_str(separator);
    }
    path.push_str(&existing);
    env.insert("PATH".to_string(), path);
    Ok(PlanTools { env })
}

#[derive(Clone)]
struct RustInstall {
    cargo_home: PathBuf,
    rustup_home: PathBuf,
    cargo_bin: PathBuf,
    linker_bin: PathBuf,
}

fn ensure_node<L>(expected: Option<&str>, mut log: L) -> Result<PathBuf, String>
where
    L: FnMut(&str, &str),
{
    if let Some(node) = node_program().filter(|path| run_output(path, &["--version"], None).is_ok()) {
        log("success", &format!("Node.js is available at {}", node.display()));
        return Ok(node);
    }
    let destination = tools_root().join("Node");
    let installed = destination.join("node.exe");
    if installed.exists() {
        return Ok(installed);
    }
    log("group", "Downloading portable Node.js");
    let releases: Vec<NodeRelease> = http_client()?
        .get("https://nodejs.org/dist/index.json")
        .header("User-Agent", "APK-Build-Launcher")
        .send()
        .map_err(display_err)?
        .error_for_status()
        .map_err(display_err)?
        .json()
        .map_err(display_err)?;
    let expected_major = expected.and_then(first_number);
    let release = releases
        .iter()
        .find(|release| {
            !release.lts.is_boolean() || release.lts.as_bool() != Some(false)
        })
        .and_then(|first_lts| {
            releases.iter().find(|release| {
                let lts = !release.lts.is_boolean() || release.lts.as_bool() != Some(false);
                let major_matches = expected_major
                    .as_deref()
                    .map(|major| release.version.trim_start_matches('v').split('.').next() == Some(major))
                    .unwrap_or(true);
                lts && major_matches
            }).or(Some(first_lts))
        })
        .ok_or_else(|| "Could not resolve a Node.js LTS release.".to_string())?;
    let url = format!(
        "https://nodejs.org/dist/{0}/node-{0}-win-x64.zip",
        release.version
    );
    let archive = download_bytes(&url, &mut log)?;
    let temp = tools_root().join("_node-extract");
    ensure_clean_dir(&temp)?;
    extract_zip_bytes(&archive, &temp)?;
    let source = single_child_dir(&temp).unwrap_or(temp.clone());
    ensure_clean_dir(&destination)?;
    copy_dir_all(&source, &destination).map_err(display_err)?;
    let _ = fs::remove_dir_all(&temp);
    if installed.exists() {
        log("success", &format!("Portable Node.js installed at {}", installed.display()));
        Ok(installed)
    } else {
        Err("Node.js was downloaded, but node.exe was not found.".to_string())
    }
}

fn ensure_rust_gnu<L>(mut log: L) -> Result<RustInstall, String>
where
    L: FnMut(&str, &str),
{
    let root = tools_root().join("Rust");
    let cargo_home = root.join("cargo");
    let rustup_home = root.join("rustup");
    let cargo_bin = cargo_home.join("bin");
    let cargo = cargo_bin.join("cargo.exe");
    let rustup = cargo_bin.join("rustup.exe");
    let linker_bin = ensure_portable_gcc(|level, message| log(level, message))?;

    if !cargo.exists() || !rustup.exists() {
        log("group", "Downloading the per-user Rust installer");
        ensure_dir(&root)?;
        let installer = root.join("rustup-init.exe");
        let bytes = download_bytes(
            "https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe",
            &mut log,
        )?;
        fs::write(&installer, bytes).map_err(display_err)?;
        let status = Command::new(&installer)
            .args([
                "-y",
                "--no-modify-path",
                "--profile",
                "minimal",
                "--default-host",
                "x86_64-pc-windows-gnu",
                "--default-toolchain",
                "stable",
            ])
            .env("CARGO_HOME", &cargo_home)
            .env("RUSTUP_HOME", &rustup_home)
            .status()
            .map_err(display_err)?;
        if !status.success() {
            return Err(format!("The non-admin Rust installer failed with status {}.", status));
        }
    }
    let rustc = rustup_home
        .join("toolchains")
        .join("stable-x86_64-pc-windows-gnu")
        .join("bin")
        .join("rustc.exe");
    if !rustc.exists() {
        let status = Command::new(&rustup)
            .args(["toolchain", "install", "stable-x86_64-pc-windows-gnu", "--profile", "minimal"])
            .env("CARGO_HOME", &cargo_home)
            .env("RUSTUP_HOME", &rustup_home)
            .status()
            .map_err(display_err)?;
        if !status.success() {
            return Err(format!("Rust GNU toolchain installation failed with status {}.", status));
        }
    }
    log("success", &format!("Rust GNU toolchain is available at {}", cargo.display()));
    Ok(RustInstall { cargo_home, rustup_home, cargo_bin, linker_bin })
}

fn ensure_portable_gcc<L>(mut log: L) -> Result<PathBuf, String>
where
    L: FnMut(&str, &str),
{
    let destination = tools_root().join("w64devkit");
    if let Some(bin) = find_child_with(&destination, "gcc.exe").and_then(|gcc| gcc.parent().map(Path::to_path_buf)) {
        ensure_libgcc_eh_alias(&bin)?;
        return Ok(bin);
    }
    log("group", "Downloading portable GCC for no-admin Windows linking");
    let release: GithubRelease = http_client()?
        .get("https://api.github.com/repos/skeeto/w64devkit/releases/latest")
        .header("User-Agent", "APK-Build-Launcher")
        .send()
        .map_err(display_err)?
        .error_for_status()
        .map_err(display_err)?
        .json()
        .map_err(display_err)?;
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name.starts_with("w64devkit-x64-") && asset.name.ends_with(".7z.exe"))
        .ok_or_else(|| "Could not find the portable x64 w64devkit archive.".to_string())?;
    let bytes = download_bytes(&asset.browser_download_url, &mut log)?;
    verify_github_digest(&bytes, asset)?;
    ensure_clean_dir(&destination)?;
    let archive = tools_root().join("w64devkit-installer.exe");
    fs::write(&archive, bytes).map_err(display_err)?;
    let output_arg = format!("-o{}", destination.display());
    let status = Command::new(&archive)
        .args(["-y", output_arg.as_str()])
        .status()
        .map_err(display_err)?;
    let _ = fs::remove_file(&archive);
    if !status.success() {
        return Err(format!("Portable GCC extraction failed with status {}.", status));
    }
    let bin = find_child_with(&destination, "gcc.exe")
        .and_then(|gcc| gcc.parent().map(Path::to_path_buf))
        .ok_or_else(|| "Portable GCC was extracted, but gcc.exe was not found.".to_string())?;
    ensure_libgcc_eh_alias(&bin)?;
    log("success", &format!("Portable GCC is available at {}", bin.display()));
    Ok(bin)
}

fn ensure_libgcc_eh_alias(bin: &Path) -> Result<(), String> {
    let root = bin.parent().unwrap_or(bin);
    if let Some(libgcc) = find_child_with(root, "libgcc.a") {
        let alias = libgcc.with_file_name("libgcc_eh.a");
        if !alias.exists() {
            fs::copy(libgcc, alias).map_err(display_err)?;
        }
    }
    Ok(())
}

fn ensure_dotnet<L>(expected: Option<&str>, mut log: L) -> Result<PathBuf, String>
where
    L: FnMut(&str, &str),
{
    if let Some(dotnet) = dotnet_program().filter(|path| run_output(path, &["--version"], None).is_ok()) {
        log("success", &format!(".NET SDK is available at {}", dotnet.display()));
        return Ok(dotnet);
    }
    let destination = tools_root().join("dotnet");
    let installed = destination.join("dotnet.exe");
    if installed.exists() {
        return Ok(installed);
    }
    log("group", "Downloading the official non-admin .NET installer");
    ensure_dir(&destination)?;
    let script = destination.join("dotnet-install.ps1");
    let bytes = download_bytes("https://dot.net/v1/dotnet-install.ps1", &mut log)?;
    fs::write(&script, bytes).map_err(display_err)?;
    let channel = expected
        .and_then(first_number)
        .map(|major| format!("{}.0", major))
        .unwrap_or_else(|| "8.0".to_string());
    let status = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            script.to_string_lossy().as_ref(),
            "-Channel",
            channel.as_str(),
            "-InstallDir",
            destination.to_string_lossy().as_ref(),
            "-NoPath",
        ])
        .status()
        .map_err(display_err)?;
    if status.success() && installed.exists() {
        log("success", &format!(".NET SDK installed at {}", installed.display()));
        Ok(installed)
    } else {
        Err(format!(".NET SDK installation failed with status {}.", status))
    }
}

pub fn ensure_git_bash<L>(mut log: L) -> Result<PathBuf, String>
where
    L: FnMut(&str, &str),
{
    if let Some(path) = cached_git_bash() {
        log("success", &format!("Git Bash is available at {}", path.display()));
        return Ok(path);
    }

    if let Some(path) = find_git_bash() {
        set_cached_git_bash(path.clone());
        log("success", &format!("Git Bash is available at {}", path.display()));
        return Ok(path);
    }
    ensure_git(|level, message| log(level, message))?;
    let bash = find_git_bash().ok_or_else(|| {
        "Portable Git was installed, but Git Bash was not found. Switch to Native Windows mode or reinstall portable Git.".to_string()
    })?;
    set_cached_git_bash(bash.clone());
    Ok(bash)
}

pub fn ensure_android_sdk<L>(compile_sdks: &[u32], java: &JavaInstall, mut log: L) -> Result<PathBuf, String>
where
    L: FnMut(&str, &str),
{
    if let Some(sdk) = cached_android_sdk().filter(|sdk| android_packages_installed(sdk, compile_sdks)) {
        log("success", &format!("Android SDK packages are already installed at {}", sdk.display()));
        return Ok(sdk);
    }

    let sdk = android_sdk_path().unwrap_or_else(|_| default_android_sdk());
    let sdkmanager = sdk.join("cmdline-tools").join("latest").join("bin").join("sdkmanager.bat");
    if !sdkmanager.exists() {
        log("group", "Downloading Android command-line tools");
        let archive = download_bytes(ANDROID_CMDLINE_TOOLS_URL, &mut log)?;
        let temp = tools_root().join("_android-cmdline-tools");
        ensure_clean_dir(&temp)?;
        extract_zip_bytes(&archive, &temp)?;
        let source = temp.join("cmdline-tools");
        let destination = sdk.join("cmdline-tools").join("latest");
        ensure_clean_dir(&destination)?;
        copy_dir_all(&source, &destination).map_err(display_err)?;
        let _ = fs::remove_dir_all(&temp);
        log("success", &format!("Android command-line tools installed at {}", destination.display()));
    }

    let mut sdks = compile_sdks.to_vec();
    if sdks.is_empty() {
        sdks.push(36);
    }
    sdks.sort();
    sdks.dedup();
    let packages = missing_android_packages(&sdk, &sdks);
    if packages.is_empty() {
        log("success", &format!("Android SDK packages are already installed at {}", sdk.display()));
        set_cached_android_sdk(sdk.clone());
        return Ok(sdk);
    }

    accept_android_licenses(&sdkmanager, &sdk, java, |level, message| log(level, message))?;
    install_android_packages(&sdkmanager, &sdk, java, &packages, |level, message| log(level, message))?;
    set_cached_android_sdk(sdk.clone());
    Ok(sdk)
}

pub fn git_program() -> Option<PathBuf> {
    let mut candidates = vec![
        PathBuf::from(r"C:\Program Files\Git\cmd\git.exe"),
        PathBuf::from(r"C:\Program Files (x86)\Git\cmd\git.exe"),
    ];
    if let Some(local_app_data) = dirs::data_local_dir() {
        candidates.insert(0, local_app_data.join("ApkBuildLauncher").join("tools").join("Git").join("cmd").join("git.exe"));
        candidates.insert(1, local_app_data.join("Programs").join("Git").join("cmd").join("git.exe"));
    }
    if let Some(path) = candidates.into_iter().find(|path| path.exists()) {
        return Some(path);
    }
    let path_git = PathBuf::from("git");
    if run_output(&path_git, &["--version"], None).is_ok() {
        Some(path_git)
    } else {
        None
    }
}

pub fn find_git_bash() -> Option<PathBuf> {
    let mut candidates = vec![
        r"C:\Program Files\Git\bin\bash.exe",
        r"C:\Program Files (x86)\Git\bin\bash.exe",
    ].into_iter().map(PathBuf::from).collect::<Vec<_>>();
    if let Some(local_app_data) = dirs::data_local_dir() {
        candidates.push(local_app_data.join("ApkBuildLauncher").join("tools").join("Git").join("bin").join("bash.exe"));
        candidates.push(local_app_data.join("Programs").join("Git").join("bin").join("bash.exe"));
    }
    candidates.into_iter().find(|path| path.exists())
}

fn accept_android_licenses<L>(
    sdkmanager: &Path,
    sdk: &Path,
    java: &JavaInstall,
    mut log: L,
) -> Result<(), String>
where
    L: FnMut(&str, &str),
{
    let sentinel = sdk.join(LICENSE_SENTINEL);
    if sentinel.exists() {
        log("info", "Android SDK licenses were already accepted locally");
        return Ok(());
    }

    log("info", "Accepting Android SDK licenses locally");
    let input = "y\n".repeat(80);
    let status = sdkmanager_command(sdkmanager, sdk, java)
        .arg("--licenses")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            if let Some(stdin) = child.stdin.as_mut() {
                stdin.write_all(input.as_bytes())?;
            }
            child.wait_with_output()
        })
        .map_err(display_err)?;
    if status.status.success() {
        fs::write(sentinel, "ok").map_err(display_err)?;
        Ok(())
    } else {
        Err(format!("Android license acceptance failed: {}", String::from_utf8_lossy(&status.stderr)))
    }
}

fn missing_android_packages(sdk: &Path, sdks: &[u32]) -> Vec<String> {
    let mut packages = vec![];
    if !sdk.join("platform-tools").join("adb.exe").exists() {
        packages.push("platform-tools".to_string());
    }
    for sdk_version in sdks {
        if !sdk.join("platforms").join(format!("android-{}", sdk_version)).join("android.jar").exists() {
            packages.push(format!("platforms;android-{}", sdk_version));
        }
        if !sdk.join("build-tools").join(format!("{}.0.0", sdk_version)).join("aapt2.exe").exists() {
            packages.push(format!("build-tools;{}.0.0", sdk_version));
        }
    }
    packages
}

fn android_packages_installed(sdk: &Path, compile_sdks: &[u32]) -> bool {
    let mut sdks = compile_sdks.to_vec();
    if sdks.is_empty() {
        sdks.push(36);
    }
    sdks.sort();
    sdks.dedup();
    missing_android_packages(sdk, &sdks).is_empty()
}

fn install_android_packages<L>(
    sdkmanager: &Path,
    sdk: &Path,
    java: &JavaInstall,
    packages: &[String],
    mut log: L,
) -> Result<(), String>
where
    L: FnMut(&str, &str),
{
    log("group", &format!("Installing Android SDK packages: {}", packages.join(", ")));
    let mut command = sdkmanager_command(sdkmanager, sdk, java);
    command.args(packages);
    let output = command.output().map_err(display_err)?;
    if output.status.success() {
        log("success", "Android SDK packages are installed");
        Ok(())
    } else {
        Err(format!(
            "Android SDK package install failed: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

fn sdkmanager_command(sdkmanager: &Path, sdk: &Path, java: &JavaInstall) -> Command {
    let mut command = Command::new(sdkmanager);
    command.arg(format!("--sdk_root={}", sdk.display()));
    command.env("ANDROID_HOME", sdk);
    command.env("ANDROID_SDK_ROOT", sdk);
    if let Some(home) = &java.home {
        command.env("JAVA_HOME", home);
        let java_bin = home.join("bin");
        let existing_path = std::env::var("PATH").unwrap_or_default();
        command.env("PATH", format!("{};{}", java_bin.display(), existing_path));
    }
    command
}

fn latest_mingit_url() -> Result<String, String> {
    let client = http_client()?;
    let release: GithubRelease = client
        .get("https://api.github.com/repos/git-for-windows/git/releases/latest")
        .header("User-Agent", "APK-Build-Launcher")
        .send()
        .map_err(display_err)?
        .error_for_status()
        .map_err(display_err)?
        .json()
        .map_err(display_err)?;

    release
        .assets
        .iter()
        .filter(|asset| asset.name.starts_with("MinGit-"))
        .filter(|asset| asset.name.ends_with("-64-bit.zip"))
        .filter(|asset| !asset.name.contains("busybox"))
        .map(|asset| asset.browser_download_url.clone())
        .next()
        .ok_or_else(|| "Could not find a 64-bit MinGit zip in the latest Git for Windows release.".to_string())
}

fn download_bytes<L>(url: &str, log: &mut L) -> Result<Vec<u8>, String>
where
    L: FnMut(&str, &str),
{
    log("info", &format!("Downloading {}", url));
    let client = http_client()?;
    let mut response = client
        .get(url)
        .header("User-Agent", "APK-Build-Launcher")
        .send()
        .map_err(display_err)?
        .error_for_status()
        .map_err(display_err)?;
    let total = response.content_length();
    let mut bytes = vec![];
    let mut buffer = [0_u8; 64 * 1024];
    let mut next_progress = 10_u64;
    loop {
        let read = response.read(&mut buffer).map_err(display_err)?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..read]);
        if let Some(total) = total.filter(|total| *total > 0) {
            let progress = (bytes.len() as u64 * 100 / total).min(100);
            if progress >= next_progress {
                log("info", &format!("Download {}% complete", progress));
                next_progress = ((progress / 10) + 1) * 10;
            }
        }
    }
    log("success", &format!("Downloaded {:.1} MB", bytes.len() as f64 / 1024.0 / 1024.0));
    Ok(bytes)
}

fn verify_github_digest(bytes: &[u8], asset: &GithubAsset) -> Result<(), String> {
    let Some(expected) = asset.digest.as_deref().and_then(|value| value.strip_prefix("sha256:")) else {
        return Ok(());
    };
    let actual = format!("{:x}", Sha256::digest(bytes));
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(format!("Checksum verification failed for {}.", asset.name))
    }
}

fn http_client() -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(900))
        .build()
        .map_err(display_err)
}

fn extract_zip_bytes(bytes: &[u8], destination: &Path) -> Result<(), String> {
    ensure_dir(destination)?;
    let reader = Cursor::new(bytes);
    let mut archive = ZipArchive::new(reader).map_err(display_err)?;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(display_err)?;
        let Some(enclosed) = file.enclosed_name().map(|path| path.to_owned()) else {
            continue;
        };
        let out_path = destination.join(enclosed);
        if file.is_dir() {
            ensure_dir(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                ensure_dir(parent)?;
            }
            let mut output = fs::File::create(&out_path).map_err(display_err)?;
            io::copy(&mut file, &mut output).map_err(display_err)?;
        }
    }
    Ok(())
}

fn find_java(expected: &str) -> Result<JavaInstall, String> {
    let expected_major = expected_major_text(expected);
    let mut detected = vec![];
    for (java, home) in java_candidates(&expected_major) {
        let output = Command::new(&java).arg("-version").output();
        let Ok(output) = output else {
            continue;
        };
        let version_text = String::from_utf8_lossy(&output.stderr).to_string() + &String::from_utf8_lossy(&output.stdout);
        let major = java_major_version(&version_text).unwrap_or_else(|| "unknown".to_string());
        if major == expected_major {
            return Ok(JavaInstall { java, home });
        }
        detected.push(format!("{} at {}", major, java.display()));
    }
    if detected.is_empty() {
        Err(format!("Java {} was not found.", expected_major))
    } else {
        Err(format!("Java {} was not found. Detected {}.", expected_major, detected.join(", ")))
    }
}

fn java_candidates(expected_major: &str) -> Vec<(PathBuf, Option<PathBuf>)> {
    let exe = if cfg!(windows) { "java.exe" } else { "java" };
    let mut candidates = vec![];
    if let Ok(java_home) = std::env::var("JAVA_HOME") {
        push_java_home(&mut candidates, PathBuf::from(java_home), exe);
    }
    let tool_root = tools_root();
    push_java_home(&mut candidates, tool_root.join(format!("jdk-{}", expected_major)), exe);
    push_matching_java_homes(&mut candidates, &tool_root, expected_major, exe);
    if let Some(local_app_data) = dirs::data_local_dir() {
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

fn push_matching_java_homes(candidates: &mut Vec<(PathBuf, Option<PathBuf>)>, root: &Path, expected_major: &str, exe: &str) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    let mut homes = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter(|path| {
            let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default().to_ascii_lowercase();
            name.contains(&format!("jdk-{}", expected_major)) || name.contains(&format!("jdk{}", expected_major)) || name.contains(&format!("-{}", expected_major))
        })
        .collect::<Vec<_>>();
    homes.sort();
    for home in homes {
        push_java_home(candidates, home, exe);
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

fn expected_major_text(expected: &str) -> String {
    let digits = expected.trim().chars().take_while(|c| c.is_ascii_digit()).collect::<String>();
    if digits.is_empty() {
        "17".to_string()
    } else {
        digits
    }
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
    let fallback = default_android_sdk();
    if fallback.exists() {
        return Ok(fallback);
    }
    Err("Android SDK was not found.".to_string())
}

fn default_android_sdk() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ApkBuildLauncher")
        .join("tools")
        .join("Android")
        .join("Sdk")
}

fn tools_root() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ApkBuildLauncher")
        .join("tools")
}

fn ensure_clean_dir(path: &Path) -> Result<(), String> {
    if path.exists() {
        fs::remove_dir_all(path).map_err(display_err)?;
    }
    ensure_dir(path)
}

fn ensure_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(display_err)
}

fn single_child_dir(path: &Path) -> Option<PathBuf> {
    let mut dirs = fs::read_dir(path)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    if dirs.len() == 1 {
        dirs.pop()
    } else {
        None
    }
}

fn copy_dir_all(source: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let target = destination.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_all(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn run_output(program: &Path, args: &[&str], envs: Option<&[(&str, &Path)]>) -> Result<String, String> {
    let mut command = Command::new(program);
    command.args(args);
    if let Some(envs) = envs {
        for (key, value) in envs {
            command.env(key, value);
        }
    }
    let output = command.output().map_err(display_err)?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

fn probe_command(
    program: Option<PathBuf>,
    args: &[&str],
    missing_message: &str,
    ready_message: &str,
) -> ToolProbe {
    program
        .filter(|path| run_output(path, args, None).is_ok())
        .map_or_else(
            || ToolProbe {
                available: false,
                path: None,
                message: missing_message.to_string(),
            },
            |path| ToolProbe {
                available: true,
                path: Some(path.display().to_string()),
                message: ready_message.to_string(),
            },
        )
}

fn node_program() -> Option<PathBuf> {
    command_candidate("node.exe", &tools_root().join("Node").join("node.exe"))
}

fn cargo_program() -> Option<PathBuf> {
    command_candidate(
        "cargo.exe",
        &tools_root().join("Rust").join("cargo").join("bin").join("cargo.exe"),
    )
}

fn dotnet_program() -> Option<PathBuf> {
    command_candidate("dotnet.exe", &tools_root().join("dotnet").join("dotnet.exe"))
}

fn command_candidate(command: &str, local: &Path) -> Option<PathBuf> {
    if local.exists() {
        return Some(local.to_path_buf());
    }
    let command_path = PathBuf::from(command);
    run_output(&command_path, &["--version"], None)
        .ok()
        .map(|_| command_path)
}

fn first_number(value: &str) -> Option<String> {
    let digits = value
        .chars()
        .skip_while(|character| !character.is_ascii_digit())
        .take_while(|character| character.is_ascii_digit())
        .collect::<String>();
    (!digits.is_empty()).then_some(digits)
}

fn find_child_with(root: &Path, file_name: &str) -> Option<PathBuf> {
    if !root.exists() {
        return None;
    }
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.file_name().and_then(|name| name.to_str()).map(|name| name.eq_ignore_ascii_case(file_name)).unwrap_or(false) {
            return Some(path);
        }
        if path.is_dir() {
            if let Some(found) = find_child_with(&path, file_name) {
                return Some(found);
            }
        }
    }
    None
}

fn cached_git() -> Option<PathBuf> {
    GIT_CACHE.get_or_init(|| Mutex::new(None)).lock().ok()?.clone()
}

fn set_cached_git(path: PathBuf) {
    if let Ok(mut cache) = GIT_CACHE.get_or_init(|| Mutex::new(None)).lock() {
        *cache = Some(path);
    }
}

fn cached_git_bash() -> Option<PathBuf> {
    GIT_BASH_CACHE.get_or_init(|| Mutex::new(None)).lock().ok()?.clone()
}

fn set_cached_git_bash(path: PathBuf) {
    if let Ok(mut cache) = GIT_BASH_CACHE.get_or_init(|| Mutex::new(None)).lock() {
        *cache = Some(path);
    }
}

fn cached_java(major: &str) -> Option<JavaInstall> {
    JAVA_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .ok()?
        .get(major)
        .cloned()
}

fn set_cached_java(major: String, java: JavaInstall) {
    if let Ok(mut cache) = JAVA_CACHE.get_or_init(|| Mutex::new(HashMap::new())).lock() {
        cache.insert(major, java);
    }
}

fn cached_android_sdk() -> Option<PathBuf> {
    ANDROID_SDK_CACHE.get_or_init(|| Mutex::new(None)).lock().ok()?.clone()
}

fn set_cached_android_sdk(path: PathBuf) {
    if let Ok(mut cache) = ANDROID_SDK_CACHE.get_or_init(|| Mutex::new(None)).lock() {
        *cache = Some(path);
    }
}

fn display_err<E: std::fmt::Display>(err: E) -> String {
    err.to_string()
}
