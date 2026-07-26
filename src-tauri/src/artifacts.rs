use crate::planning::ArtifactRule;
use glob::glob;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishedArtifact {
    pub kind: String,
    pub path: String,
    pub relative_path: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildManifest {
    pub build_id: String,
    pub source: String,
    pub revision: String,
    pub target: String,
    pub created_at: String,
    pub output_folder: String,
    pub log_file: Option<String>,
    pub artifacts: Vec<PublishedArtifact>,
}

pub fn collect(
    workspace: &Path,
    output: &Path,
    rules: &[ArtifactRule],
) -> Result<Vec<PublishedArtifact>, String> {
    fs::create_dir_all(output).map_err(display_err)?;
    let canonical_workspace = workspace.canonicalize().map_err(display_err)?;
    let artifacts_root = output.join("artifacts");
    fs::create_dir_all(&artifacts_root).map_err(display_err)?;
    let mut seen = HashSet::new();
    let mut published = vec![];

    for rule in rules {
        let pattern = workspace
            .join(rule.pattern.trim())
            .to_string_lossy()
            .replace('\\', "/");
        for entry in glob(&pattern).map_err(display_err)? {
            let path = entry.map_err(display_err)?;
            if path.is_dir() {
                collect_directory(
                    &canonical_workspace,
                    &path,
                    &artifacts_root,
                    &rule.kind,
                    &mut seen,
                    &mut published,
                )?;
            } else if path.is_file() {
                publish_file(
                    &canonical_workspace,
                    &path,
                    &artifacts_root,
                    &rule.kind,
                    &mut seen,
                    &mut published,
                )?;
            }
        }
    }
    published.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(published)
}

pub fn write_manifest(output: &Path, manifest: &BuildManifest) -> Result<PathBuf, String> {
    let path = output.join("build-manifest.json");
    let text = serde_json::to_string_pretty(manifest).map_err(display_err)?;
    fs::write(&path, text).map_err(display_err)?;
    Ok(path)
}

pub fn publish_latest(final_output: &Path, latest: &Path) -> Result<(), String> {
    let parent = latest.parent().ok_or_else(|| "Latest folder has no parent.".to_string())?;
    fs::create_dir_all(parent).map_err(display_err)?;
    let staging = parent.join(format!(
        ".{}-staging",
        latest.file_name().unwrap_or_default().to_string_lossy()
    ));
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(display_err)?;
    }
    copy_tree(final_output, &staging)?;
    if latest.exists() {
        fs::remove_dir_all(latest).map_err(display_err)?;
    }
    fs::rename(staging, latest).map_err(display_err)
}

pub fn read_history(source_root: &Path, limit: usize) -> Result<Vec<BuildManifest>, String> {
    if !source_root.exists() {
        return Ok(vec![]);
    }
    let pattern = source_root
        .join("**")
        .join("build-manifest.json")
        .to_string_lossy()
        .replace('\\', "/");
    let mut manifests = vec![];
    for entry in glob(&pattern).map_err(display_err)? {
        let path = entry.map_err(display_err)?;
        if path.components().any(|component| component.as_os_str() == "latest") {
            continue;
        }
        if let Ok(text) = fs::read_to_string(path) {
            if let Ok(manifest) = serde_json::from_str::<BuildManifest>(&text) {
                manifests.push(manifest);
            }
        }
    }
    manifests.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    manifests.truncate(limit);
    Ok(manifests)
}

fn collect_directory(
    workspace: &Path,
    directory: &Path,
    artifacts_root: &Path,
    kind: &str,
    seen: &mut HashSet<PathBuf>,
    published: &mut Vec<PublishedArtifact>,
) -> Result<(), String> {
    for entry in fs::read_dir(directory).map_err(display_err)? {
        let path = entry.map_err(display_err)?.path();
        if path.is_dir() {
            collect_directory(workspace, &path, artifacts_root, kind, seen, published)?;
        } else if path.is_file() {
            publish_file(workspace, &path, artifacts_root, kind, seen, published)?;
        }
    }
    Ok(())
}

fn publish_file(
    workspace: &Path,
    path: &Path,
    artifacts_root: &Path,
    kind: &str,
    seen: &mut HashSet<PathBuf>,
    published: &mut Vec<PublishedArtifact>,
) -> Result<(), String> {
    let canonical = path.canonicalize().map_err(display_err)?;
    if !canonical.starts_with(workspace) || !seen.insert(canonical.clone()) {
        return Ok(());
    }
    let relative = canonical.strip_prefix(workspace).map_err(display_err)?;
    let destination = artifacts_root.join(relative);
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(display_err)?;
    }
    fs::copy(&canonical, &destination).map_err(display_err)?;
    let metadata = fs::metadata(&destination).map_err(display_err)?;
    published.push(PublishedArtifact {
        kind: if kind == "file" {
            destination
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or("file")
                .to_ascii_lowercase()
        } else {
            kind.to_string()
        },
        path: destination.display().to_string(),
        relative_path: relative.to_string_lossy().replace('\\', "/"),
        size_bytes: metadata.len(),
        sha256: file_sha256(&destination)?,
    });
    Ok(())
}

fn file_sha256(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(display_err)?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(display_err)?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination).map_err(display_err)?;
    for entry in fs::read_dir(source).map_err(display_err)? {
        let entry = entry.map_err(display_err)?;
        let target = destination.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target).map_err(display_err)?;
        }
    }
    Ok(())
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
        let path = std::env::temp_dir().join(format!("abl-artifacts-{}-{}", name, nonce));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn artifacts_preserve_relative_paths_and_publish_checksums() {
        let root = temp("collect");
        let workspace = root.join("workspace");
        let output = root.join("output");
        fs::create_dir_all(workspace.join("one")).unwrap();
        fs::create_dir_all(workspace.join("two")).unwrap();
        fs::write(workspace.join("one").join("app.exe"), "first").unwrap();
        fs::write(workspace.join("two").join("app.exe"), "second").unwrap();
        let rules = vec![ArtifactRule { pattern: "**/*.exe".to_string(), kind: "exe".to_string(), required: true }];

        let published = collect(&workspace, &output, &rules).unwrap();
        assert_eq!(published.len(), 2);
        assert_ne!(published[0].path, published[1].path);
        assert!(published.iter().all(|artifact| artifact.sha256.len() == 64));
        fs::remove_dir_all(root).unwrap();
    }
}
