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
