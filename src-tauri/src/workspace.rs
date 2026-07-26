use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceMode {
    #[default]
    Remote,
    Local,
}

pub struct IsolatedWorkspace {
    path: PathBuf,
    keep: bool,
}

impl IsolatedWorkspace {
    pub fn create(source: &Path, workspaces_root: &Path, build_id: &str) -> Result<Self, String> {
        if !source.is_dir() {
            return Err(format!("Source folder does not exist: {}", source.display()));
        }
        fs::create_dir_all(workspaces_root).map_err(display_err)?;
        let path = workspaces_root.join(sanitize(build_id));
        if path.exists() {
            fs::remove_dir_all(&path).map_err(display_err)?;
        }
        fs::create_dir_all(&path).map_err(display_err)?;
        copy_snapshot(source, &path)?;
        Ok(Self { path, keep: false })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn keep_for_diagnostics(&mut self) {
        self.keep = true;
    }

    pub fn cleanup(mut self) -> Result<(), String> {
        self.keep = true;
        if self.path.exists() {
            fs::remove_dir_all(&self.path).map_err(display_err)?;
        }
        Ok(())
    }
}

impl Drop for IsolatedWorkspace {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

pub fn validate_local_source(path: &str) -> Result<PathBuf, String> {
    let source = PathBuf::from(path.trim());
    if source.as_os_str().is_empty() {
        return Err("Choose an existing local project folder.".to_string());
    }
    let canonical = source
        .canonicalize()
        .map_err(|err| format!("Could not open local project '{}': {}", source.display(), err))?;
    if !canonical.is_dir() {
        return Err(format!("Local project is not a folder: {}", canonical.display()));
    }
    Ok(canonical)
}

fn copy_snapshot(source: &Path, destination: &Path) -> Result<(), String> {
    for entry in fs::read_dir(source).map_err(display_err)? {
        let entry = entry.map_err(display_err)?;
        let name = entry.file_name();
        let name_text = name.to_string_lossy();
        if should_exclude(&name_text) {
            continue;
        }
        let source_path = entry.path();
        let destination_path = destination.join(&name);
        let file_type = entry.file_type().map_err(display_err)?;
        if file_type.is_dir() {
            fs::create_dir_all(&destination_path).map_err(display_err)?;
            copy_snapshot(&source_path, &destination_path)?;
        } else if file_type.is_file() {
            fs::copy(&source_path, &destination_path).map_err(display_err)?;
        }
    }
    Ok(())
}

fn should_exclude(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        ".git"
            | ".gradle"
            | ".idea"
            | ".vs"
            | ".tools"
            | "build"
            | "dist"
            | "node_modules"
            | "out"
            | "release"
            | "target"
    )
}

fn sanitize(input: &str) -> String {
    input
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect()
}

fn display_err<E: std::fmt::Display>(error: E) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp(name: &str) -> PathBuf {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("abl-workspace-{}-{}", name, nonce));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn snapshot_includes_local_changes_but_excludes_stale_build_outputs() {
        let root = temp("snapshot");
        let source = root.join("source");
        let workspaces = root.join("workspaces");
        fs::create_dir_all(source.join("src")).unwrap();
        fs::create_dir_all(source.join("app").join("build").join("outputs")).unwrap();
        fs::write(source.join("src").join("local-change.txt"), "not committed").unwrap();
        fs::write(source.join("app").join("build").join("outputs").join("stale.apk"), "stale").unwrap();

        let workspace = IsolatedWorkspace::create(&source, &workspaces, "build-1").unwrap();
        assert!(workspace.path().join("src").join("local-change.txt").exists());
        assert!(!workspace.path().join("app").join("build").exists());
        workspace.cleanup().unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}
