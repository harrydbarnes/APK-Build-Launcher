# APK Build Launcher

Windows desktop app for building Android and Windows application artifacts locally, without consuming GitHub Actions quota.

## Supported local targets

- Android Gradle projects: `.apk` and `.aab`
- Tauri projects: NSIS `.exe` and WiX `.msi`
- Electron projects: packaged Windows `.exe`
- .NET projects: self-contained Windows `.exe`

The launcher can analyse a practical subset of GitHub Actions workflows or generate a native plan directly from the project. Native plans are recommended because they are predictable and do not attempt to emulate the whole Actions platform.

Supported workflow adapters include:

- `actions/checkout`
- `actions/cache` using persistent local caches
- `actions/setup-java`
- `actions/setup-node`
- Rust toolchain actions
- `actions/setup-dotnet`
- `actions/upload-artifact`, including multiline paths
- standard `run` steps, step environment variables, working directories, and common Windows/Bash shells

Unknown build actions, unsupported conditions, matrix jobs, and non-Windows runners are shown as blockers before tools are downloaded. Release, publish, deploy, and store-upload actions are skipped by the local build-only safety policy.

Build commands still run with the current Windows user's permissions. Review the displayed plan and only build source and workflow files you trust.

## Seamless build flow

1. Choose an existing local folder or a remote Git repository.
2. Leave the target on **Detect automatically**, or select one explicitly.
3. Optionally select a workflow and job.
4. Select **Analyse Plan** to review compatibility, tools, secrets, commands, and expected artifacts.
5. Select **Build locally**.

The Build action analyses the plan, installs only missing tools, creates an isolated source snapshot, runs the commands, verifies fresh artifacts, writes SHA-256 checksums and a build manifest, updates `latest`, and opens persistent history in the Artifacts screen.

Local-folder builds include uncommitted source changes but exclude `.git`, dependency folders, and old build output. The selected project is never modified. Remote builds use an app-managed clean cache before creating the isolated snapshot.

## Corporate laptops and no-admin operation

All managed tools and caches live in user-writable folders beneath:

```text
%LOCALAPPDATA%\ApkBuildLauncher\
```

The app never modifies the system `PATH` and does not require elevation:

- Portable Git / Git Bash from Git for Windows
- Portable Temurin JDK
- Android command-line tools and project-specific SDK packages
- Portable Node.js LTS
- Rustup installed into the app tool cache
- The official Rust `x86_64-pc-windows-gnu` toolchain
- Portable w64devkit GCC, avoiding the Visual Studio Build Tools admin dependency
- The official per-user `.NET` installer

Existing tools on `PATH`, `JAVA_HOME`, `ANDROID_HOME`, or `ANDROID_SDK_ROOT` are reused when suitable. Downloads report progress. GitHub release digests are verified when the release API provides one.

Corporate TLS inspection or an authenticated proxy can still prevent vendor downloads. In that case, IT can allow-list:

```text
api.github.com
github.com
static.rust-lang.org
nodejs.org
api.adoptium.net
download.visualstudio.microsoft.com
dl.google.com
dot.net
```

Once tools and dependencies are cached, repeat builds reuse them locally.

## Artifacts and history

Final outputs are written to:

```text
<output>\<project>\<branch-or-local>\<timestamp>\
<output>\<project>\latest\
```

Each timestamped build contains:

- `artifacts\` with paths preserved to avoid filename collisions
- `build-manifest.json`
- target and source revision
- artifact sizes and SHA-256 checksums
- the persistent build-log path

Artifacts are never uploaded.

## Secrets

- Required secret names are derived from the selected plan.
- Values are stored per source using Windows DPAPI.
- Every stored secret value is redacted from streamed and persistent command output.
- Local property files and generated workspaces are removed after the build.
- Publish and deployment steps are disabled in local build mode.

## Development

With an existing Rust/MSVC toolchain:

```powershell
npm ci
npm run tauri dev
npm run tauri build
```

Without admin rights or Visual Studio Build Tools:

```powershell
npm run test:core
npm run build:non-admin
```

The bootstrap script installs Rust and portable GCC beneath the ignored repository `.tools` folder, makes no system changes, runs the core tests, and can build the Windows installers.

## Continuous integration

GitHub Actions is now release-only: it runs for version tags or manual dispatch, rather than every push to `main`. Normal development and validation should use the local non-admin build route.
