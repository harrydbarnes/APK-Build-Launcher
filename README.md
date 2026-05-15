# APK Build Launcher

A Windows desktop app that runs Android APK build workflows locally — no CI minutes, no waiting for a remote runner, no admin rights required.

It reads your existing GitHub Actions workflow files and executes the relevant steps right on your machine, turning a full CI pipeline into a one-click local build.

## Stack

- **Tauri v2** — Rust backend, native WebView frontend
- **React + TypeScript + Vite** — UI
- **Tailwind CSS** — styling, with system / light / dark theme support
- Rust backend handles cloning, workflow parsing, secret management, tool bootstrapping, process execution, and artifact copying

## What It Does

### Workflow Adapter

The core engine is the Workflow Adapter — a targeted runner, not a full GitHub Actions clone. It supports the subset of Actions that Android APK workflows actually use:

- `actions/checkout@v4`
- `actions/setup-java@v4`
- `actions/upload-artifact@v4`
- Standard `run` steps
- Workflow, job, and step `env`
- `${{ secrets.NAME }}`, `${{ env.NAME }}`, `${{ github.workspace }}`, and `${{ github.ref_name }}` expressions

**Native Windows mode** covers most Android workflows directly:

- Decodes base64 keystore secrets into `local.properties` and `local.dev.properties`
- Applies `build.gradle.kts` edits for signing config
- Translates `./gradlew` → `.\gradlew.bat`
- Renames and copies APK artifacts to the chosen output folder

**Git Bash mode** is available for workflows with more complex shell scripts, provided Git for Windows is installed (or auto-installed by the app).

### Views

| View | What you get |
|------|-------------|
| **Home** | Build setup, one-click presets, readiness checks, secrets entry |
| **Workflows** | Browse and select detected workflows and jobs from `.github/workflows` |
| **Logs** | Live build console with search, level filter, auto-scroll, and cancel |
| **Artifacts** | List of copied APKs with output path, latest path, and copy buttons |
| **Settings** | Default folders, shell mode, theme, and tool install / repair |

### Presets

Save any build setup as a named preset — repo, branch, workflow, job, output folder, and shell mode are all included. Presets can be renamed, duplicated, set as default, and updated in place. The default preset loads automatically on launch, making repeat builds genuinely one click.

### Branch and PR Support

The branch field accepts branch names, PR branch names, or PR numbers (e.g. `123`). Hit the **Branches** button or press Enter in the URL field to pull the available branches for the repo.

## No Admin Rights Required

The app is built for locked-down Windows 10/11 work laptops. The NSIS `.exe` installer does a current-user install with no elevation needed. Missing build tools are downloaded automatically to `%LOCALAPPDATA%\ApkBuildLauncher\tools`:

- **Git / Git Bash** — latest MinGit from Git for Windows
- **Java** — portable Temurin/OpenJDK from Adoptium (triggered by `actions/setup-java`)
- **Android SDK** — Google command-line tools, then `sdkmanager` installs `platform-tools`, the detected `compileSdk` platform, and matching build tools

The app checks `PATH`, `JAVA_HOME`, `ANDROID_HOME`, `ANDROID_SDK_ROOT`, `%LOCALAPPDATA%\Programs\Git`, and common per-user Java locations before downloading anything — existing installs are reused. Gradle lives under `%LOCALAPPDATA%\ApkBuildLauncher\gradle` with caching, parallel execution, and the daemon enabled.

Repeat builds skip re-downloading tools, skip already-installed SDK packages, skip licence acceptance after the first run, and skip re-fetching a repo when the selected ref is already checked out.

## Local Paths

| Purpose | Path |
|---------|------|
| Cloned repos | `%LOCALAPPDATA%\ApkBuildLauncher\repos\` |
| Config and presets | `%APPDATA%\ApkBuildLauncher\` |
| Build tools | `%LOCALAPPDATA%\ApkBuildLauncher\tools\` |
| Gradle cache | `%LOCALAPPDATA%\ApkBuildLauncher\gradle\` |
| Timestamped APKs | `<output-folder>\<repo-name>\<branch>\<timestamp>\` |
| Latest APKs | `<output-folder>\<repo-name>\latest\` |

The default repo folder is configurable in Settings.

## Security

- Secrets are stored per repo using Windows DPAPI.
- Build logs redact `LOCAL_PROPERTIES_BASE64`, `LOCAL_DEV_PROPERTIES_BASE64`, and any `local.properties` / `local.dev.properties` references.
- `local.properties` and `local.dev.properties` are added to `.git/info/exclude` so they are never accidentally committed.
- Artifacts stay local — nothing is uploaded.

## Development

Install Node/npm and the Rust toolchain, then:

```powershell
npm install
npm run tauri dev
```

Build installers:

```powershell
npm run tauri build
```

## Continuous Integration

GitHub Actions builds the Windows desktop launcher on every push to `main` and uploads the MSI and NSIS installers as workflow artifacts. The repository does not contain an Android project, so CI produces the launcher itself — the app that then builds your APKs locally.
