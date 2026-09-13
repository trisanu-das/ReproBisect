use std::{
    ffi::OsStr,
    fs::{self, File},
    io::{BufReader, Read},
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use walkdir::{DirEntry, WalkDir};

use crate::model::{
    DependencyFileRecord, DependencyResolutionRecord, SourceCopyOrder, SourceProvenance,
};

const DEFAULT_IGNORES: &[&str] = &[".git", ".reprobisect", "target"];

const DEPENDENCY_MANIFEST_NAMES: &[&str] = &[
    "Cargo.lock",
    "package-lock.json",
    "npm-shrinkwrap.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "bun.lock",
    "bun.lockb",
    "poetry.lock",
    "Pipfile.lock",
    "uv.lock",
    "requirements.txt",
    "requirements.lock",
    "go.sum",
    "vendor/modules.txt",
    "Gemfile.lock",
    "composer.lock",
    "mix.lock",
    "gradle.lockfile",
];

/// Collect source-control identity and hashes of common dependency lock/manifest
/// files. For supported ecosystems (Cargo, npm/pnpm/yarn, Python lockfiles,
/// Go, Bundler, Composer, and Gradle), also persist only a privacy-preserving
/// package-count plus a hash of normalized name/version coordinates. These records describe the declared dependency snapshot; they
/// are not proof that a build resolved no additional network inputs.
pub fn collect_source_provenance(root: &Path) -> Result<SourceProvenance> {
    let (git_commit, git_dirty) = git_provenance(root);
    let (dependency_files, dependency_resolutions) = collect_dependency_provenance(root)?;

    Ok(SourceProvenance {
        git_commit,
        git_dirty,
        dependency_files,
        dependency_resolutions,
    })
}

/// Collect only dependency declaration/resolution provenance from a source or
/// fresh build workspace. This is used after a build to capture lockfiles that
/// package managers may have generated or updated.
pub fn collect_dependency_provenance(
    root: &Path,
) -> Result<(Vec<DependencyFileRecord>, Vec<DependencyResolutionRecord>)> {
    let mut dependency_files = Vec::new();

    for (relative, absolute) in collect_entries(root)? {
        let basename = relative.file_name().and_then(|name| name.to_str()).unwrap_or("");
        let matches = DEPENDENCY_MANIFEST_NAMES.iter().any(|candidate| {
            if candidate.contains('/') {
                relative.ends_with(Path::new(candidate))
            } else {
                basename == *candidate
            }
        });
        if !matches {
            continue;
        }

        let metadata = fs::symlink_metadata(&absolute)
            .with_context(|| format!("cannot stat dependency manifest {}", absolute.display()))?;
        if !metadata.file_type().is_file() {
            continue;
        }
        dependency_files.push(DependencyFileRecord {
            path: relative,
            sha256: sha256_file(&absolute)?,
            size_bytes: metadata.len(),
        });
    }

    dependency_files.sort_by(|left, right| left.path.cmp(&right.path));
    let mut dependency_resolutions = dependency_files
        .iter()
        .filter_map(|record| parse_dependency_resolution(root, &record.path))
        .collect::<Vec<_>>();
    dependency_resolutions.sort_by(|left, right| left.path.cmp(&right.path));

    Ok((dependency_files, dependency_resolutions))
}

fn parse_dependency_resolution(root: &Path, relative: &Path) -> Option<DependencyResolutionRecord> {
    let name = relative.file_name()?.to_str()?;
    let absolute = root.join(relative);
    let (ecosystem, parsed) = match name {
        "Cargo.lock" => ("cargo", parse_cargo_lock(&absolute)),
        "package-lock.json" | "npm-shrinkwrap.json" => ("npm", parse_npm_lock(&absolute)),
        "pnpm-lock.yaml" => ("pnpm", parse_pnpm_lock(&absolute)),
        "yarn.lock" => ("yarn", parse_yarn_lock(&absolute)),
        "poetry.lock" => ("poetry", parse_poetry_lock(&absolute)),
        "Pipfile.lock" => ("pipenv", parse_pipfile_lock(&absolute)),
        "uv.lock" => ("uv", parse_uv_lock(&absolute)),
        "requirements.txt" | "requirements.lock" => {
            ("python-requirements", parse_requirements_lock(&absolute))
        }
        "go.sum" => ("go", parse_go_sum(&absolute)),
        "modules.txt" if relative.ends_with(Path::new("vendor/modules.txt")) => {
            ("go-vendor", parse_go_vendor_modules(&absolute))
        }
        "Gemfile.lock" => ("bundler", parse_gemfile_lock(&absolute)),
        "composer.lock" => ("composer", parse_composer_lock(&absolute)),
        "gradle.lockfile" => ("gradle", parse_gradle_lock(&absolute)),
        _ => return None,
    };

    Some(match parsed {
        Ok(coordinates) => DependencyResolutionRecord {
            path: relative.to_path_buf(),
            ecosystem: ecosystem.to_string(),
            parsed: true,
            package_count: coordinates.len(),
            normalized_sha256: Some(hash_normalized_coordinates(&coordinates)),
            error: None,
        },
        Err(_error) => DependencyResolutionRecord {
            path: relative.to_path_buf(),
            ecosystem: ecosystem.to_string(),
            parsed: false,
            package_count: 0,
            normalized_sha256: None,
            // Never persist parser diagnostics: parser errors may include private
            // dependency names, registry URLs, or source snippets.
            error: Some(format!("{ecosystem} dependency declaration could not be parsed")),
        },
    })
}

fn parse_cargo_lock(path: &Path) -> Result<Vec<String>> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("cannot read Cargo lockfile {}", path.display()))?;
    let value: toml::Value = toml::from_str(&raw)
        .with_context(|| format!("cannot parse Cargo lockfile {}", path.display()))?;
    let packages = value
        .get("package")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("Cargo.lock has no package array"))?;
    let mut coordinates = Vec::new();
    for package in packages {
        let Some(table) = package.as_table() else { continue };
        let Some(name) = table.get("name").and_then(toml::Value::as_str) else { continue };
        let Some(version) = table.get("version").and_then(toml::Value::as_str) else { continue };
        coordinates.push(format!("{name}@{version}"));
    }
    coordinates.sort();
    Ok(coordinates)
}

fn parse_npm_lock(path: &Path) -> Result<Vec<String>> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("cannot read npm lockfile {}", path.display()))?;
    let value: serde_json::Value = serde_json::from_str(&raw)
        .with_context(|| format!("cannot parse npm lockfile {}", path.display()))?;
    let mut coordinates = Vec::new();
    if let Some(packages) = value.get("packages").and_then(serde_json::Value::as_object) {
        for (package_path, package) in packages {
            if package_path.is_empty() {
                continue;
            }
            if let Some(version) = package.get("version").and_then(serde_json::Value::as_str) {
                coordinates.push(format!("{package_path}@{version}"));
            }
        }
    } else if let Some(dependencies) = value.get("dependencies").and_then(serde_json::Value::as_object) {
        collect_npm_v1_dependencies("", dependencies, &mut coordinates);
    } else {
        bail!("npm lockfile has neither packages nor dependencies");
    }
    coordinates.sort();
    Ok(coordinates)
}

fn collect_npm_v1_dependencies(
    prefix: &str,
    dependencies: &serde_json::Map<String, serde_json::Value>,
    out: &mut Vec<String>,
) {
    for (name, dependency) in dependencies {
        let qualified = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if let Some(version) = dependency.get("version").and_then(serde_json::Value::as_str) {
            out.push(format!("{qualified}@{version}"));
        }
        if let Some(children) = dependency.get("dependencies").and_then(serde_json::Value::as_object) {
            collect_npm_v1_dependencies(&qualified, children, out);
        }
    }
}

fn parse_toml_package_array(path: &Path) -> Result<Vec<String>> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("cannot read TOML lockfile {}", path.display()))?;
    let value: toml::Value = toml::from_str(&raw)
        .with_context(|| format!("cannot parse TOML lockfile {}", path.display()))?;
    let packages = value
        .get("package")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("lockfile has no package array"))?;
    let mut coordinates = Vec::new();
    for package in packages {
        let Some(table) = package.as_table() else { continue };
        let Some(name) = table.get("name").and_then(toml::Value::as_str) else { continue };
        let Some(version) = table.get("version").and_then(toml::Value::as_str) else { continue };
        coordinates.push(format!("{name}@{version}"));
    }
    coordinates.sort();
    coordinates.dedup();
    Ok(coordinates)
}

fn parse_poetry_lock(path: &Path) -> Result<Vec<String>> {
    parse_toml_package_array(path)
}

fn parse_uv_lock(path: &Path) -> Result<Vec<String>> {
    parse_toml_package_array(path)
}

fn parse_pipfile_lock(path: &Path) -> Result<Vec<String>> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("cannot read Pipfile.lock {}", path.display()))?;
    let value: serde_json::Value = serde_json::from_str(&raw)
        .with_context(|| format!("cannot parse Pipfile.lock {}", path.display()))?;
    let mut coordinates = Vec::new();
    for section in ["default", "develop"] {
        let Some(packages) = value.get(section).and_then(serde_json::Value::as_object) else {
            continue;
        };
        for (name, package) in packages {
            let version = package
                .get("version")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("<unversioned>");
            coordinates.push(format!("{name}@{version}"));
        }
    }
    if coordinates.is_empty() {
        bail!("Pipfile.lock has no default/develop package entries");
    }
    coordinates.sort();
    coordinates.dedup();
    Ok(coordinates)
}

fn parse_requirements_lock(path: &Path) -> Result<Vec<String>> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("cannot read requirements file {}", path.display()))?;
    let mut coordinates = raw
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter(|line| !line.starts_with('-'))
        .map(|line| line.split(" #").next().unwrap_or(line).trim().to_string())
        .filter(|line| line.contains("=="))
        .collect::<Vec<_>>();
    coordinates.sort();
    coordinates.dedup();
    Ok(coordinates)
}

fn parse_go_sum(path: &Path) -> Result<Vec<String>> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("cannot read go.sum {}", path.display()))?;
    let mut coordinates = Vec::new();
    for line in raw.lines() {
        let mut fields = line.split_whitespace();
        let Some(module) = fields.next() else { continue };
        let Some(version) = fields.next() else { continue };
        if fields.next().is_none() {
            continue;
        }
        let version = version.strip_suffix("/go.mod").unwrap_or(version);
        coordinates.push(format!("{module}@{version}"));
    }
    coordinates.sort();
    coordinates.dedup();
    Ok(coordinates)
}

fn parse_go_vendor_modules(path: &Path) -> Result<Vec<String>> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("cannot read vendor/modules.txt {}", path.display()))?;
    let mut coordinates = Vec::new();
    for line in raw.lines().filter(|line| line.starts_with("# ")) {
        let mut fields = line[2..].split_whitespace();
        let Some(module) = fields.next() else { continue };
        let Some(version) = fields.next() else { continue };
        if version.starts_with('v') {
            coordinates.push(format!("{module}@{version}"));
        }
    }
    coordinates.sort();
    coordinates.dedup();
    Ok(coordinates)
}

fn parse_composer_lock(path: &Path) -> Result<Vec<String>> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("cannot read composer.lock {}", path.display()))?;
    let value: serde_json::Value = serde_json::from_str(&raw)
        .with_context(|| format!("cannot parse composer.lock {}", path.display()))?;
    let mut coordinates = Vec::new();
    for section in ["packages", "packages-dev"] {
        let Some(packages) = value.get(section).and_then(serde_json::Value::as_array) else {
            continue;
        };
        for package in packages {
            let Some(name) = package.get("name").and_then(serde_json::Value::as_str) else { continue };
            let Some(version) = package.get("version").and_then(serde_json::Value::as_str) else { continue };
            coordinates.push(format!("{name}@{version}"));
        }
    }
    coordinates.sort();
    coordinates.dedup();
    Ok(coordinates)
}

fn parse_gradle_lock(path: &Path) -> Result<Vec<String>> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("cannot read gradle.lockfile {}", path.display()))?;
    let mut coordinates = raw
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with("empty="))
        .filter_map(|line| line.split_once('=').map(|(coordinate, _)| coordinate.trim().to_string()))
        .filter(|coordinate| coordinate.matches(':').count() >= 2)
        .collect::<Vec<_>>();
    coordinates.sort();
    coordinates.dedup();
    Ok(coordinates)
}

fn parse_gemfile_lock(path: &Path) -> Result<Vec<String>> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("cannot read Gemfile.lock {}", path.display()))?;
    let mut in_specs = false;
    let mut coordinates = Vec::new();
    for line in raw.lines() {
        if line.trim() == "specs:" {
            in_specs = true;
            continue;
        }
        if in_specs && !line.starts_with(' ') {
            in_specs = false;
        }
        if !in_specs || !line.starts_with("    ") || line.starts_with("      ") {
            continue;
        }
        let entry = line.trim();
        let Some(open) = entry.rfind(" (") else { continue };
        let Some(version) = entry.strip_suffix(')').map(|value| &value[open + 2..]) else { continue };
        let name = &entry[..open];
        if !name.is_empty() && !version.is_empty() {
            coordinates.push(format!("{name}@{version}"));
        }
    }
    coordinates.sort();
    coordinates.dedup();
    Ok(coordinates)
}

fn parse_yarn_lock(path: &Path) -> Result<Vec<String>> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("cannot read yarn.lock {}", path.display()))?;
    let mut current: Option<String> = None;
    let mut coordinates = Vec::new();
    for line in raw.lines() {
        if !line.chars().next().is_some_and(char::is_whitespace) && line.trim_end().ends_with(':') {
            current = Some(line.trim().trim_end_matches(':').trim_matches('"').to_string());
            continue;
        }
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("resolution:") {
            coordinates.push(value.trim().trim_matches('"').to_string());
            current = None;
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("version ") {
            if let Some(selector) = current.take() {
                coordinates.push(format!("{}@{}", selector, value.trim().trim_matches('"')));
            }
        }
    }
    if coordinates.is_empty() {
        bail!("yarn.lock contained no parseable package entries");
    }
    coordinates.sort();
    coordinates.dedup();
    Ok(coordinates)
}

fn parse_pnpm_lock(path: &Path) -> Result<Vec<String>> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("cannot read pnpm-lock.yaml {}", path.display()))?;
    let mut in_packages = false;
    let mut found_packages_section = false;
    let mut coordinates = Vec::new();
    for line in raw.lines() {
        if !line.starts_with(' ') {
            let key = line.trim();
            in_packages = key == "packages:" || (!found_packages_section && key == "snapshots:");
            if key == "packages:" {
                found_packages_section = true;
                coordinates.clear();
            }
            continue;
        }
        if !in_packages {
            continue;
        }
        let leading = line.len() - line.trim_start().len();
        if leading != 2 {
            continue;
        }
        let trimmed = line.trim();
        if !trimmed.ends_with(':') {
            continue;
        }
        let key = trimmed.trim_end_matches(':').trim_matches('"').trim_matches('\'');
        if key.contains('@') || key.starts_with('/') {
            coordinates.push(key.to_string());
        }
    }
    if coordinates.is_empty() {
        bail!("pnpm lockfile contained no parseable package entries");
    }
    coordinates.sort();
    coordinates.dedup();
    Ok(coordinates)
}

fn hash_normalized_coordinates(coordinates: &[String]) -> String {
    let mut hasher = Sha256::new();
    for coordinate in coordinates {
        hasher.update(coordinate.as_bytes());
        hasher.update(b"\0");
    }
    hex::encode(hasher.finalize())
}

fn git_provenance(root: &Path) -> (Option<String>, Option<bool>) {
    let commit = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|value| !value.is_empty());

    let dirty = commit.as_ref().and_then(|_| {
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["status", "--porcelain", "--untracked-files=normal"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| !output.stdout.is_empty())
    });

    (commit, dirty)
}

pub(crate) fn sha256_file(path: &Path) -> Result<String> {
    let file = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .with_context(|| format!("cannot read {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}


pub fn digest_tree(root: &Path) -> Result<String> {
    let mut entries = collect_entries(root)?;
    entries.sort_by(|left, right| left.0.cmp(&right.0));

    let mut hasher = Sha256::new();

    for (relative, absolute) in entries {
        let metadata = fs::symlink_metadata(&absolute)
            .with_context(|| format!("cannot stat {}", absolute.display()))?;
        let relative_bytes = relative.to_string_lossy();

        if metadata.file_type().is_symlink() {
            hasher.update(b"L\0");
            hasher.update(relative_bytes.as_bytes());
            hasher.update(b"\0");
            let target = fs::read_link(&absolute)
                .with_context(|| format!("cannot read symlink {}", absolute.display()))?;
            hasher.update(target.to_string_lossy().as_bytes());
            hasher.update(b"\0");
            continue;
        }

        if metadata.is_dir() {
            hasher.update(b"D\0");
            hasher.update(relative_bytes.as_bytes());
            hasher.update(b"\0");
            hash_unix_mode(&mut hasher, &metadata);
            continue;
        }

        if metadata.is_file() {
            hasher.update(b"F\0");
            hasher.update(relative_bytes.as_bytes());
            hasher.update(b"\0");
            hasher.update(metadata.len().to_le_bytes());
            hasher.update(b"\0");

            hash_unix_mode(&mut hasher, &metadata);

            let file = File::open(&absolute)
                .with_context(|| format!("cannot open {}", absolute.display()))?;
            let mut reader = BufReader::new(file);
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                let read = reader
                    .read(&mut buffer)
                    .with_context(|| format!("cannot read {}", absolute.display()))?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }
            hasher.update(b"\0");
        }
    }

    Ok(hex::encode(hasher.finalize()))
}

pub fn copy_source_tree(
    source: &Path,
    destination: &Path,
    order: SourceCopyOrder,
) -> Result<()> {
    fs::create_dir_all(destination)
        .with_context(|| format!("cannot create {}", destination.display()))?;

    let mut entries = collect_copy_entries(source)?;
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    if order == SourceCopyOrder::Reverse {
        entries.reverse();
    }

    let mut directory_permissions = Vec::new();

    for (relative, absolute, kind) in entries {
        let target = destination.join(&relative);

        if kind.is_dir() {
            fs::create_dir_all(&target)
                .with_context(|| format!("cannot create {}", target.display()))?;
            directory_permissions.push((target.clone(), fs::metadata(&absolute)?.permissions()));
        } else if kind.is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&absolute, &target).with_context(|| {
                format!(
                    "cannot copy {} to {}",
                    absolute.display(),
                    target.display()
                )
            })?;
            let permissions = fs::metadata(&absolute)?.permissions();
            fs::set_permissions(&target, permissions)?;
        } else if kind.is_symlink() {
            copy_symlink(&absolute, &target)?;
        }
    }

    for (directory, permissions) in directory_permissions.into_iter().rev() {
        fs::set_permissions(&directory, permissions)
            .with_context(|| format!("cannot set permissions on {}", directory.display()))?;
    }

    Ok(())
}

fn collect_copy_entries(source: &Path) -> Result<Vec<(PathBuf, PathBuf, fs::FileType)>> {
    let mut entries = Vec::new();
    for entry in WalkDir::new(source).follow_links(false) {
        let entry = entry.with_context(|| format!("cannot walk {}", source.display()))?;
        if should_skip(&entry, source) {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(source)
            .with_context(|| format!("cannot relativize {}", entry.path().display()))?
            .to_path_buf();
        if relative.as_os_str().is_empty() {
            continue;
        }
        entries.push((relative, entry.path().to_path_buf(), entry.file_type()));
    }
    Ok(entries)
}

fn hash_unix_mode(hasher: &mut Sha256, metadata: &fs::Metadata) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        hasher.update((metadata.permissions().mode() & 0o7777).to_le_bytes());
    }

    #[cfg(not(unix))]
    {
        let _ = (hasher, metadata);
    }
}

fn collect_entries(root: &Path) -> Result<Vec<(PathBuf, PathBuf)>> {
    if !root.is_dir() {
        bail!("source root {} is not a directory", root.display());
    }

    let mut entries = Vec::new();
    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry.with_context(|| format!("cannot walk {}", root.display()))?;
        if should_skip(&entry, root) {
            continue;
        }

        let relative = entry
            .path()
            .strip_prefix(root)
            .with_context(|| format!("cannot relativize {}", entry.path().display()))?
            .to_path_buf();

        if relative.as_os_str().is_empty() {
            continue;
        }
        entries.push((relative, entry.path().to_path_buf()));
    }
    Ok(entries)
}

fn should_skip(entry: &DirEntry, root: &Path) -> bool {
    if entry.path() == root {
        return false;
    }

    let relative = match entry.path().strip_prefix(root) {
        Ok(value) => value,
        Err(_) => return false,
    };

    relative.components().any(|component| {
        let value = component.as_os_str();
        DEFAULT_IGNORES.iter().any(|ignored| value == OsStr::new(ignored))
    })
}

#[cfg(unix)]
fn copy_symlink(source: &Path, target: &Path) -> Result<()> {
    use std::os::unix::fs::symlink;

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let link_target = fs::read_link(source)
        .with_context(|| format!("cannot read symlink {}", source.display()))?;
    symlink(link_target, target)
        .with_context(|| format!("cannot create symlink {}", target.display()))?;
    Ok(())
}

#[cfg(not(unix))]
fn copy_symlink(source: &Path, target: &Path) -> Result<()> {
    // v0.1 is Linux-first. This fallback preserves content when the host is not Unix.
    let resolved = fs::canonicalize(source)
        .with_context(|| format!("cannot resolve symlink {}", source.display()))?;
    if resolved.is_file() {
        fs::copy(resolved, target)?;
        Ok(())
    } else {
        bail!("directory symlink copying is unsupported on this host")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_is_stable_and_content_sensitive() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("a.txt"), "alpha").unwrap();
        fs::create_dir(temp.path().join("nested")).unwrap();
        fs::write(temp.path().join("nested/b.txt"), "beta").unwrap();

        let first = digest_tree(temp.path()).unwrap();
        let second = digest_tree(temp.path()).unwrap();
        assert_eq!(first, second);

        fs::write(temp.path().join("nested/b.txt"), "changed").unwrap();
        let third = digest_tree(temp.path()).unwrap();
        assert_ne!(first, third);
    }

    #[test]
    fn copy_order_changes_creation_sequence_without_changing_bytes() {
        let source = tempfile::tempdir().unwrap();
        fs::write(source.path().join("a.txt"), "alpha").unwrap();
        fs::write(source.path().join("b.txt"), "beta").unwrap();

        let sorted = tempfile::tempdir().unwrap();
        let reverse = tempfile::tempdir().unwrap();
        copy_source_tree(source.path(), sorted.path(), SourceCopyOrder::Sorted).unwrap();
        copy_source_tree(source.path(), reverse.path(), SourceCopyOrder::Reverse).unwrap();

        assert_eq!(fs::read(sorted.path().join("a.txt")).unwrap(), fs::read(reverse.path().join("a.txt")).unwrap());
        assert_eq!(fs::read(sorted.path().join("b.txt")).unwrap(), fs::read(reverse.path().join("b.txt")).unwrap());
    }

    #[test]
    fn digest_ignores_target_and_git() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("source.txt"), "same").unwrap();
        let first = digest_tree(temp.path()).unwrap();

        fs::create_dir(temp.path().join("target")).unwrap();
        fs::write(temp.path().join("target/generated"), "noise").unwrap();
        fs::create_dir(temp.path().join(".git")).unwrap();
        fs::write(temp.path().join(".git/index"), "noise").unwrap();

        let second = digest_tree(temp.path()).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn source_provenance_hashes_dependency_files() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("Cargo.lock"), "package = demo\n").unwrap();
        fs::write(temp.path().join("source.txt"), "source\n").unwrap();
        let provenance = collect_source_provenance(temp.path()).unwrap();
        assert_eq!(provenance.dependency_files.len(), 1);
        assert_eq!(provenance.dependency_files[0].path, PathBuf::from("Cargo.lock"));
        assert_eq!(provenance.dependency_files[0].sha256.len(), 64);
    }

    #[test]
    fn source_provenance_parses_resolution_without_persisting_coordinates() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("Cargo.lock"),
            r#"version = 3

[[package]]
name = "demo"
version = "1.2.3"

[[package]]
name = "helper"
version = "4.5.6"
"#,
        )
        .unwrap();
        fs::write(
            temp.path().join("requirements.txt"),
            "alpha==1.0\nbeta==2.0 # pinned\n--index-url https://private.invalid/simple\n",
        )
        .unwrap();
        let provenance = collect_source_provenance(temp.path()).unwrap();
        assert_eq!(provenance.dependency_resolutions.len(), 2);
        let cargo = provenance
            .dependency_resolutions
            .iter()
            .find(|record| record.ecosystem == "cargo")
            .unwrap();
        assert!(cargo.parsed);
        assert_eq!(cargo.package_count, 2);
        assert_eq!(cargo.normalized_sha256.as_deref().unwrap().len(), 64);
        let requirements = provenance
            .dependency_resolutions
            .iter()
            .find(|record| record.ecosystem == "python-requirements")
            .unwrap();
        assert_eq!(requirements.package_count, 2);
    }

    #[test]
    fn parses_multiple_dependency_ecosystems_without_persisting_coordinates() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("poetry.lock"),
            "[[package]]\nname = \"alpha\"\nversion = \"1.0.0\"\n",
        )
        .unwrap();
        fs::write(
            temp.path().join("uv.lock"),
            "version = 1\n[[package]]\nname = \"beta\"\nversion = \"2.0.0\"\n",
        )
        .unwrap();
        fs::write(
            temp.path().join("Pipfile.lock"),
            r#"{"default":{"gamma":{"version":"==3.0.0"}},"develop":{}}"#,
        )
        .unwrap();
        fs::write(
            temp.path().join("go.sum"),
            "example.com/mod v1.2.3 h1:abc\nexample.com/mod v1.2.3/go.mod h1:def\n",
        )
        .unwrap();
        fs::write(
            temp.path().join("composer.lock"),
            r#"{"packages":[{"name":"vendor/pkg","version":"1.0.0"}],"packages-dev":[]}"#,
        )
        .unwrap();
        fs::write(
            temp.path().join("gradle.lockfile"),
            "org.example:demo:1.2.3=runtimeClasspath\n",
        )
        .unwrap();
        fs::write(
            temp.path().join("Gemfile.lock"),
            "GEM\n  specs:\n    rack (3.0.0)\n\nPLATFORMS\n  ruby\n",
        )
        .unwrap();
        fs::write(
            temp.path().join("yarn.lock"),
            "alpha@^1.0.0:\n  version \"1.2.0\"\n",
        )
        .unwrap();
        fs::write(
            temp.path().join("pnpm-lock.yaml"),
            "lockfileVersion: '9.0'\npackages:\n  alpha@1.2.0:\n    resolution: {integrity: sha512-demo}\n",
        )
        .unwrap();

        let provenance = collect_source_provenance(temp.path()).unwrap();
        let ecosystems = provenance
            .dependency_resolutions
            .iter()
            .filter(|record| record.parsed)
            .map(|record| record.ecosystem.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        for expected in ["poetry", "uv", "pipenv", "go", "composer", "gradle", "bundler", "yarn", "pnpm"] {
            assert!(ecosystems.contains(expected), "missing {expected}: {ecosystems:?}");
        }
        assert!(provenance.dependency_resolutions.iter().all(|record| {
            !record.parsed || record.normalized_sha256.as_deref().is_some_and(|hash| hash.len() == 64)
        }));
    }

    #[cfg(unix)]
    #[test]
    fn source_provenance_does_not_follow_dependency_symlinks() {
        use std::os::unix::fs::symlink;

        let project = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        fs::write(outside.path(), "outside secret-like bytes\n").unwrap();
        symlink(outside.path(), project.path().join("requirements.txt")).unwrap();

        let provenance = collect_source_provenance(project.path()).unwrap();
        assert!(provenance.dependency_files.is_empty());
    }

}
