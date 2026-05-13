# APK Build Launcher

Windows desktop app for locally building Android APKs from a practical subset of GitHub Actions workflows.

## Stack

- Tauri v2
- React
- TypeScript
- Vite
- Tailwind CSS
- Rust backend commands for cloning, workflow parsing, secret handling, process execution, and artifact copying

## V1 Scope

The internal engine is the Workflow Adapter. It is intentionally not a complete GitHub Actions runner. V1 focuses on Android APK workflows that use:

- `actions/checkout@v4`
- `actions/setup-java@v4`
- `actions/upload-artifact@v4`
- standard `run` steps
- workflow, job, and step `env`
- simple `${{ secrets.NAME }}`, `${{ env.NAME }}`, `${{ github.workspace }}`, and `${{ github.ref_name }}` expressions

Native Windows mode handles the target Android workflow directly where possible:

- decodes base64 local property secrets into `local.properties` and `local.dev.properties`
- applies the sample `build.gradle.kts` edits
- translates `./gradlew` to `.\gradlew.bat`
- renames and copies APK artifacts locally

Git Bash mode is available for more complex Bash scripts when Git for Windows is installed.

## Local Paths

- Repos: `%LOCALAPPDATA%\ApkBuildLauncher\repos\`
- Config: `%APPDATA%\ApkBuildLauncher\`
- Final APKs: `<chosen-output-folder>\<repo-name>\<branch>\<timestamp>\`
- Latest APKs: `<chosen-output-folder>\<repo-name>\latest\`

The default repo folder can be changed in Settings. The backend honors that folder when cloning and updating repositories.

## Windows 10/11 Without Admin Rights

The app is designed to run from user-writable locations on locked-down work laptops. Use the NSIS `.exe` installer, which is configured for current-user installation. If a required build tool is missing, the launcher downloads and installs it under `%LOCALAPPDATA%\ApkBuildLauncher\tools` without requiring admin rights:

- Portable Git / Git Bash: downloaded from the latest Git for Windows MinGit release when cloning or Bash mode needs it.
- Java: downloaded as a portable Temurin/OpenJDK JDK from Adoptium when a workflow uses `actions/setup-java`.
- Android SDK: downloaded from Google's Android command-line tools package, then `sdkmanager` installs `platform-tools`, the detected `compileSdk` platform, and matching build tools.

The Tools tab shows what is available and has an Install / Repair Tools button. The clone, branch lookup, and build flows also bootstrap missing tools automatically.

Existing per-user installs still work. The app checks `PATH`, `JAVA_HOME`, `ANDROID_HOME`, `ANDROID_SDK_ROOT`, `%LOCALAPPDATA%\Programs\Git`, and common per-user Java locations before downloading anything. Gradle downloads are kept under `%LOCALAPPDATA%\ApkBuildLauncher\gradle`.

## Security Notes

- Secrets are stored per repo.
- On Windows, saved secrets use DPAPI.
- Logs redact `LOCAL_PROPERTIES_BASE64`, `LOCAL_DEV_PROPERTIES_BASE64`, `local.properties`, and `local.dev.properties` references.
- `local.properties` and `local.dev.properties` are added to `.git/info/exclude`.
- Artifacts are never uploaded.

## Development

Install Node/npm and the Rust toolchain, then run:

```powershell
npm install
npm run tauri dev
```

Build installers:

```powershell
npm run tauri build
```

## Continuous Integration

GitHub Actions builds the Windows desktop launcher on every push to `main` and uploads the MSI/NSIS installers as workflow artifacts. This repository does not contain an Android project or a configured Tauri mobile target, so CI builds the launcher that creates APKs locally rather than an Android `.apk` for the launcher itself.
