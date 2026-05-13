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

The app is designed to run from user-writable locations. For a laptop where you cannot install machine-wide tools, use per-user installs:

- Git for Windows: install for the current user, or make sure `git.exe` is in `PATH`. The app also checks `%LOCALAPPDATA%\Programs\Git\cmd\git.exe`.
- Portable Git: unpack Git for Windows to `%LOCALAPPDATA%\ApkBuildLauncher\tools\Git`. The app checks `cmd\git.exe` for clone and branch lookup, and `bin\bash.exe` for Git Bash mode.
- Git Bash mode: install Git for Windows for the current user, or use the portable Git location above.
- Java: install JDK 17 and set `JAVA_HOME`, make sure `java.exe` is in `PATH`, or unpack a JDK to `%LOCALAPPDATA%\ApkBuildLauncher\tools\jdk-17`.
- Android SDK: set `ANDROID_HOME` or `ANDROID_SDK_ROOT`, or install the SDK at `%LOCALAPPDATA%\Android\Sdk`.

Cloning and branch lookup only require Git. Java 17 is checked when a build actually needs Java, and the app passes the detected JDK into build steps as `JAVA_HOME` and at the front of `PATH`.

When the SDK is found under `%LOCALAPPDATA%\Android\Sdk`, the app passes that path to build steps as both `ANDROID_HOME` and `ANDROID_SDK_ROOT`.

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
