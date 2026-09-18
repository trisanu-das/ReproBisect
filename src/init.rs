use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde_json::Value as JsonValue;
use toml::Value as TomlValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectionConfidence {
    High,
    Medium,
    Low,
}

impl DetectionConfidence {
    pub fn label(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProjectKind {
    Cargo,
    Go,
    NodePnpm,
    NodeNpm,
    Maven,
    Gradle,
    Bazel,
    Meson,
    CMake,
    Autotools,
    Python,
    Make,
    Generic,
}

impl ProjectKind {
    fn label(self) -> &'static str {
        match self {
            Self::Cargo => "Cargo",
            Self::Go => "Go",
            Self::NodePnpm => "Node.js / pnpm",
            Self::NodeNpm => "Node.js / npm",
            Self::Maven => "Maven",
            Self::Gradle => "Gradle",
            Self::Bazel => "Bazel",
            Self::Meson => "Meson",
            Self::CMake => "CMake",
            Self::Autotools => "Autotools",
            Self::Python => "Python packaging",
            Self::Make => "Make",
            Self::Generic => "generic / unknown",
        }
    }
}

#[derive(Debug, Clone)]
pub struct InitPlan {
    pub detected_project: String,
    pub confidence: DetectionConfidence,
    pub image: String,
    pub command: Vec<String>,
    pub outputs: Vec<PathBuf>,
    pub notes: Vec<String>,
}

pub fn create_config(project: &Path, force: bool) -> Result<InitPlan> {
    let path = project.join(".reprobisect.toml");
    if path.exists() && !force {
        bail!(
            "{} already exists; pass --force to overwrite it",
            path.display()
        );
    }

    let plan = detect_project(project);
    fs::write(&path, render_config(&plan))
        .with_context(|| format!("cannot write {}", path.display()))?;

    println!("created {}", path.display());
    println!(
        "detected project: {} ({} confidence)",
        plan.detected_project,
        plan.confidence.label()
    );
    println!("image: {}", plan.image);
    println!("command: {}", display_command(&plan.command));
    println!(
        "likely output{}: {}",
        if plan.outputs.len() == 1 { "" } else { "s" },
        plan.outputs
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    for note in &plan.notes {
        println!("note: {note}");
    }
    println!("next: review .reprobisect.toml, then run reprobisect doctor .");

    Ok(plan)
}

pub fn detect_project(project: &Path) -> InitPlan {
    let kinds = detected_kinds(project);
    let selected = kinds.first().copied().unwrap_or(ProjectKind::Generic);
    let mut plan = plan_for(project, selected);

    if kinds.len() > 1 {
        let alternatives = kinds[1..]
            .iter()
            .map(|kind| kind.label())
            .collect::<Vec<_>>()
            .join(", ");
        plan.notes.push(format!(
            "multiple build-system markers were found; selected {} by deterministic precedence, also found {alternatives}",
            selected.label()
        ));
        if plan.confidence == DetectionConfidence::High {
            plan.confidence = DetectionConfidence::Medium;
        }
    }

    plan
}

fn detected_kinds(project: &Path) -> Vec<ProjectKind> {
    let mut kinds = Vec::new();
    if has(project, "Cargo.toml") {
        kinds.push(ProjectKind::Cargo);
    }
    if has(project, "go.mod") {
        kinds.push(ProjectKind::Go);
    }
    if has(project, "package.json") && has(project, "pnpm-lock.yaml") {
        kinds.push(ProjectKind::NodePnpm);
    } else if has(project, "package.json") {
        kinds.push(ProjectKind::NodeNpm);
    }
    if has(project, "pom.xml") {
        kinds.push(ProjectKind::Maven);
    }
    if any(
        project,
        &[
            "gradlew",
            "build.gradle",
            "build.gradle.kts",
            "settings.gradle",
            "settings.gradle.kts",
        ],
    ) {
        kinds.push(ProjectKind::Gradle);
    }
    if any(
        project,
        &["MODULE.bazel", "WORKSPACE", "WORKSPACE.bazel", "BUILD", "BUILD.bazel"],
    ) {
        kinds.push(ProjectKind::Bazel);
    }
    if has(project, "meson.build") {
        kinds.push(ProjectKind::Meson);
    }
    if has(project, "CMakeLists.txt") {
        kinds.push(ProjectKind::CMake);
    }
    if any(project, &["configure.ac", "configure.in", "configure"]) {
        kinds.push(ProjectKind::Autotools);
    }
    if any(project, &["pyproject.toml", "setup.py", "setup.cfg"]) {
        kinds.push(ProjectKind::Python);
    }
    if any(project, &["GNUmakefile", "Makefile", "makefile"]) {
        kinds.push(ProjectKind::Make);
    }
    kinds
}

fn plan_for(project: &Path, kind: ProjectKind) -> InitPlan {
    match kind {
        ProjectKind::Cargo => cargo_plan(project),
        ProjectKind::Go => go_plan(project),
        ProjectKind::NodePnpm => node_plan(project, true),
        ProjectKind::NodeNpm => node_plan(project, false),
        ProjectKind::Maven => maven_plan(project),
        ProjectKind::Gradle => gradle_plan(project),
        ProjectKind::Bazel => bazel_plan(project),
        ProjectKind::Meson => meson_plan(project),
        ProjectKind::CMake => cmake_plan(project),
        ProjectKind::Autotools => autotools_plan(project),
        ProjectKind::Python => python_plan(),
        ProjectKind::Make => make_plan(project),
        ProjectKind::Generic => generic_plan(),
    }
}

fn cargo_plan(project: &Path) -> InitPlan {
    let value = read(project, "Cargo.toml")
        .and_then(|text| text.parse::<TomlValue>().ok());
    let name = value
        .as_ref()
        .and_then(cargo_binary_name)
        .unwrap_or_else(|| "app".to_string());
    let rust = value
        .as_ref()
        .and_then(|value| value.get("package"))
        .and_then(TomlValue::as_table)
        .and_then(|table| table.get("rust-version"))
        .and_then(TomlValue::as_str)
        .and_then(version_prefix)
        .unwrap_or_else(|| "1.85".to_string());

    let mut command = vec!["cargo".into(), "build".into(), "--release".into()];
    if has(project, "Cargo.lock") {
        command.push("--locked".into());
    }

    let inferred = name != "app";
    InitPlan {
        detected_project: "Cargo".into(),
        confidence: if inferred {
            DetectionConfidence::High
        } else {
            DetectionConfidence::Medium
        },
        image: format!("rust:{rust}"),
        command,
        outputs: vec![PathBuf::from(format!("target/release/{name}"))],
        notes: vec![if inferred {
            "binary output inferred from Cargo.toml; workspace or library-only projects may need an output adjustment".into()
        } else {
            "no binary target was inferred; edit build.outputs if target/release/app is not produced".into()
        }],
    }
}

fn cargo_binary_name(value: &TomlValue) -> Option<String> {
    if let Some(bins) = value.get("bin").and_then(TomlValue::as_array) {
        if let Some(name) = bins
            .iter()
            .filter_map(TomlValue::as_table)
            .filter_map(|table| table.get("name"))
            .filter_map(TomlValue::as_str)
            .map(clean_name)
            .find(|name| !name.is_empty())
        {
            return Some(name);
        }
    }
    value
        .get("package")
        .and_then(TomlValue::as_table)
        .and_then(|table| table.get("name"))
        .and_then(TomlValue::as_str)
        .map(clean_name)
        .filter(|name| !name.is_empty())
}

fn go_plan(project: &Path) -> InitPlan {
    let version = read(project, "go.mod")
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("go "))
        .and_then(version_prefix)
        .unwrap_or_else(|| "1.24".to_string());
    let root_main = fs::read_dir(project)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().extension().and_then(|x| x.to_str()) == Some("go"))
        .filter_map(|entry| fs::read_to_string(entry.path()).ok())
        .any(|text| text.lines().any(|line| line.trim() == "package main"));

    if root_main {
        InitPlan {
            detected_project: "Go".into(),
            confidence: DetectionConfidence::High,
            image: format!("golang:{version}"),
            command: vec![
                "sh".into(),
                "-lc".into(),
                "mkdir -p build && go build -trimpath -o build/reprobisect-app .".into(),
            ],
            outputs: vec![PathBuf::from("build/reprobisect-app")],
            notes: vec!["root package appears executable; go build is directed to a stable artifact path".into()],
        }
    } else {
        InitPlan {
            detected_project: "Go".into(),
            confidence: DetectionConfidence::Medium,
            image: format!("golang:{version}"),
            command: vec![
                "sh".into(),
                "-lc".into(),
                "mkdir -p build && go test -c -o build/reprobisect-package.test .".into(),
            ],
            outputs: vec![PathBuf::from("build/reprobisect-package.test")],
            notes: vec!["root package is not clearly executable; a compiled test binary is used as the default artifact".into()],
        }
    }
}

fn node_plan(project: &Path, pnpm: bool) -> InitPlan {
    let has_build = read(project, "package.json")
        .and_then(|text| serde_json::from_str::<JsonValue>(&text).ok())
        .and_then(|value| value.get("scripts").cloned())
        .and_then(|value| value.get("build").cloned())
        .and_then(|value| value.as_str().map(str::to_string))
        .is_some();

    let install = if pnpm {
        "corepack enable && pnpm install --frozen-lockfile"
    } else if has(project, "package-lock.json") {
        "npm ci"
    } else {
        "npm install"
    };
    let build = if has_build {
        if pnpm { " && pnpm run build" } else { " && npm run build" }
    } else {
        ""
    };
    let pack = if pnpm {
        "pnpm pack --pack-destination /tmp/reprobisect-package >/dev/null"
    } else {
        "npm pack --pack-destination /tmp/reprobisect-package >/dev/null"
    };
    let shell = format!(
        "rm -rf /tmp/reprobisect-package && mkdir -p /tmp/reprobisect-package build && {install}{build} && {pack} && cp \"$(find /tmp/reprobisect-package -maxdepth 1 -type f -name '*.tgz' | sort | head -n 1)\" build/reprobisect-package.tgz"
    );

    InitPlan {
        detected_project: if pnpm { "Node.js / pnpm".into() } else { "Node.js / npm".into() },
        confidence: DetectionConfidence::High,
        image: "node:22".into(),
        command: vec!["sh".into(), "-lc".into(), shell],
        outputs: vec![PathBuf::from("build/reprobisect-package.tgz")],
        notes: vec![if has_build {
            "build script detected; init runs it before producing a package tarball at a stable path".into()
        } else {
            "no build script detected; the package tarball is used as the default reproducibility artifact".into()
        }],
    }
}

fn maven_plan(project: &Path) -> InitPlan {
    let pom = read(project, "pom.xml").unwrap_or_default();
    let artifact = xml_tag(&pom, "artifactId").map(clean_name);
    let version = xml_tag(&pom, "version").map(clean_name);
    let exact = artifact.is_some() && version.is_some();
    let output = match (artifact, version) {
        (Some(artifact), Some(version)) => format!("target/{artifact}-{version}.jar"),
        _ => "target/app.jar".into(),
    };
    InitPlan {
        detected_project: "Maven".into(),
        confidence: if exact { DetectionConfidence::Medium } else { DetectionConfidence::Low },
        image: "maven:3.9-eclipse-temurin-21".into(),
        command: vec!["mvn".into(), "-B".into(), "-DskipTests".into(), "package".into()],
        outputs: vec![PathBuf::from(output)],
        notes: vec!["review the inferred output for multi-module builds, inherited versions, WARs, or classifiers".into()],
    }
}

fn gradle_plan(project: &Path) -> InitPlan {
    let wrapper = has(project, "gradlew");
    InitPlan {
        detected_project: "Gradle".into(),
        confidence: DetectionConfidence::Low,
        image: "eclipse-temurin:21-jdk".into(),
        command: if wrapper {
            vec!["sh".into(), "-lc".into(), "./gradlew --no-daemon build".into()]
        } else {
            vec!["gradle".into(), "--no-daemon".into(), "build".into()]
        },
        outputs: vec![PathBuf::from("build/libs/app.jar")],
        notes: vec!["Gradle output naming is project-specific; edit build.outputs to the concrete produced artifact".into()],
    }
}

fn bazel_plan(project: &Path) -> InitPlan {
    let build = read(project, "BUILD.bazel").or_else(|| read(project, "BUILD")).unwrap_or_default();
    let target = bazel_binary(&build).map(clean_name).filter(|name| !name.is_empty());
    let (command, output, confidence) = if let Some(target) = target {
        (
            vec!["bazel".into(), "build".into(), format!("//:{target}")],
            format!("bazel-bin/{target}"),
            DetectionConfidence::Medium,
        )
    } else {
        (
            vec!["bazel".into(), "build".into(), "//...".into()],
            "bazel-bin/app".into(),
            DetectionConfidence::Low,
        )
    };
    InitPlan {
        detected_project: "Bazel".into(),
        confidence,
        image: "gcr.io/bazel-public/bazel:8.0.0".into(),
        command,
        outputs: vec![PathBuf::from(output)],
        notes: vec!["review the selected target/output for non-root targets and multi-target workspaces".into()],
    }
}

fn meson_plan(project: &Path) -> InitPlan {
    let target = read(project, "meson.build")
        .and_then(|text| quoted_call_arg(&text, "executable"))
        .map(clean_name)
        .filter(|name| !name.is_empty());
    let (output, confidence) = target
        .map(|target| (format!("build/{target}"), DetectionConfidence::High))
        .unwrap_or_else(|| ("build/app".into(), DetectionConfidence::Low));
    InitPlan {
        detected_project: "Meson".into(),
        confidence,
        image: "gcc:14".into(),
        command: vec![
            "sh".into(),
            "-lc".into(),
            "meson setup build --buildtype=release && meson compile -C build".into(),
        ],
        outputs: vec![PathBuf::from(output)],
        notes: vec!["verify the selected image contains compatible meson and ninja tools; edit build.image if needed".into()],
    }
}

fn cmake_plan(project: &Path) -> InitPlan {
    let target = read(project, "CMakeLists.txt")
        .and_then(|text| call_arg(&text, "add_executable"))
        .map(clean_name)
        .filter(|name| !name.is_empty());
    let (output, confidence) = target
        .map(|target| (format!("build/{target}"), DetectionConfidence::High))
        .unwrap_or_else(|| ("build/app".into(), DetectionConfidence::Low));
    InitPlan {
        detected_project: "CMake".into(),
        confidence,
        image: "gcc:14".into(),
        command: vec![
            "sh".into(),
            "-lc".into(),
            "cmake -S . -B build -DCMAKE_BUILD_TYPE=Release && cmake --build build --config Release".into(),
        ],
        outputs: vec![PathBuf::from(output)],
        notes: vec!["verify the selected image contains a compatible cmake executable; edit build.image if needed".into()],
    }
}

fn autotools_plan(project: &Path) -> InitPlan {
    let target = read(project, "Makefile.am")
        .and_then(|text| assignment_first_word(&text, "bin_PROGRAMS"))
        .map(clean_name)
        .filter(|name| !name.is_empty());
    let (output, confidence) = target
        .map(|target| (target, DetectionConfidence::Medium))
        .unwrap_or_else(|| ("app".into(), DetectionConfidence::Low));
    let configure = if has(project, "configure") {
        "./configure && make -j1"
    } else {
        "autoreconf -fi && ./configure && make -j1"
    };
    InitPlan {
        detected_project: "Autotools".into(),
        confidence,
        image: "gcc:14".into(),
        command: vec!["sh".into(), "-lc".into(), configure.into()],
        outputs: vec![PathBuf::from(output)],
        notes: vec!["verify the selected image contains the Autotools utilities required by the project".into()],
    }
}

fn python_plan() -> InitPlan {
    InitPlan {
        detected_project: "Python packaging".into(),
        confidence: DetectionConfidence::High,
        image: "python:3.12".into(),
        command: vec![
            "sh".into(),
            "-lc".into(),
            "rm -rf /tmp/reprobisect-wheel && mkdir -p /tmp/reprobisect-wheel build && python -m pip wheel . --no-deps -w /tmp/reprobisect-wheel && cp \"$(find /tmp/reprobisect-wheel -maxdepth 1 -type f -name '*.whl' | sort | head -n 1)\" build/reprobisect-artifact.whl".into(),
        ],
        outputs: vec![PathBuf::from("build/reprobisect-artifact.whl")],
        notes: vec!["a wheel is used as the default artifact and copied to a stable path; edit the command for sdist-only or installer projects".into()],
    }
}

fn make_plan(project: &Path) -> InitPlan {
    let text = read(project, "GNUmakefile")
        .or_else(|| read(project, "Makefile"))
        .or_else(|| read(project, "makefile"))
        .unwrap_or_default();
    let target = make_target(&text).map(clean_name).filter(|name| !name.is_empty());
    let (output, confidence) = target
        .map(|target| (target, DetectionConfidence::Medium))
        .unwrap_or_else(|| ("build/app".into(), DetectionConfidence::Low));
    InitPlan {
        detected_project: "Make".into(),
        confidence,
        image: "gcc:14".into(),
        command: vec!["sh".into(), "-lc".into(), "make -j1".into()],
        outputs: vec![PathBuf::from(output)],
        notes: vec!["Makefiles can produce several artifacts; review the inferred build.outputs value".into()],
    }
}

fn generic_plan() -> InitPlan {
    InitPlan {
        detected_project: "generic / unknown".into(),
        confidence: DetectionConfidence::Low,
        image: "gcc:14".into(),
        command: vec!["sh".into(), "-lc".into(), "make".into()],
        outputs: vec![PathBuf::from("build/app")],
        notes: vec!["no supported build-system marker was found; image, command, and output are placeholders".into()],
    }
}

pub fn render_config(plan: &InitPlan) -> String {
    let mut out = String::new();
    out.push_str("# Generated by reprobisect init. Review build.image, build.command, and build.outputs before the first check.\n");
    out.push_str(&format!(
        "# Detected project: {} ({} confidence)\n",
        plan.detected_project,
        plan.confidence.label()
    ));
    for note in &plan.notes {
        out.push_str("# Note: ");
        out.push_str(&note.replace('\n', " "));
        out.push('\n');
    }
    out.push_str("\n[build]\n");
    out.push_str("runner = \"docker\" # or \"podman\"\n");
    out.push_str(&format!("image = {}\n", string_value(&plan.image)));
    out.push_str(&format!("command = {}\n", string_array(&plan.command)));
    let outputs = plan
        .outputs
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    out.push_str(&format!("outputs = {}\n", string_array(&outputs)));
    out.push_str("timeout_seconds = 600\n");
    out.push_str("log_capture_max_bytes = 1048576\n\n");
    out.push_str(
        r#"[experiments]
control_runs = 2
intervention_runs = 1
comparison_runs = 2
comparison_subset_runs = 2
confirmation_runs = 1
stochastic_runs = 4
stochastic_alpha = 0.05
image_variants = []
network_trace = false
file_input_trace = false
syscall_trace_max_bytes = 33554432
runtime_dependency_provenance = false
dependency_cache_max_files = 2048
dependency_cache_max_bytes = 134217728

# Optional custom package-manager cache roots:
# [experiments.dependency_cache_paths]
# cargo = "/root/.cargo/registry/cache"

# Narrow same-image toolchain experiment example:
# [experiments.toolchain_variables]
# CC = ["gcc", "clang"]

# One-file dependency declaration experiment example:
# [[experiments.dependency_variants]]
# id = "candidate-lockfile"
# target = "Cargo.lock"
# variant_file = ".reprobisect-variants/Cargo.lock"

[experiments.dimensions]
network_access = false
source_path = false
build_path = true
source_date_epoch = true
timezone = true
locale = true
hostname = true
source_mtime = false
cpu_count = false
umask = false
directory_order = false
"#,
    );
    out
}

fn has(project: &Path, name: &str) -> bool {
    project.join(name).is_file()
}

fn any(project: &Path, names: &[&str]) -> bool {
    names.iter().any(|name| has(project, name))
}

fn read(project: &Path, name: &str) -> Option<String> {
    fs::read_to_string(project.join(name)).ok()
}

fn version_prefix(input: &str) -> Option<String> {
    let value = input.trim();
    let start = value.find(|ch: char| ch.is_ascii_digit())?;
    let value = &value[start..];
    let end = value
        .find(|ch: char| !(ch.is_ascii_digit() || ch == '.'))
        .unwrap_or(value.len());
    let version = value[..end].trim_end_matches('.');
    (!version.is_empty()).then(|| version.to_string())
}

fn clean_name(input: &str) -> String {
    input
        .trim()
        .trim_matches(|ch| ch == '"' || ch == '\'')
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '+' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn call_arg(text: &str, function: &str) -> Option<String> {
    let needle = format!("{function}(");
    let start = text.find(&needle)? + needle.len();
    text[start..]
        .split(|ch: char| ch.is_whitespace() || ch == ')' || ch == ',')
        .map(str::trim)
        .find(|part| !part.is_empty())
        .map(|part| part.trim_matches(|ch| ch == '"' || ch == '\'').to_string())
}

fn quoted_call_arg(text: &str, function: &str) -> Option<String> {
    let needle = format!("{function}(");
    let start = text.find(&needle)? + needle.len();
    let rest = text[start..].trim_start();
    let quote = rest.chars().next()?;
    if quote != '\'' && quote != '"' {
        return call_arg(text, function);
    }
    let tail = &rest[1..];
    let end = tail.find(quote)?;
    Some(tail[..end].to_string())
}

fn bazel_binary(text: &str) -> Option<String> {
    for rule in ["cc_binary", "rust_binary", "go_binary", "py_binary", "java_binary"] {
        let needle = format!("{rule}(");
        let Some(start) = text.find(&needle) else {
            continue;
        };
        let window = &text[start..text.len().min(start + 1024)];
        let Some(name_at) = window.find("name") else {
            continue;
        };
        let after = &window[name_at + 4..];
        let Some(eq) = after.find('=') else {
            continue;
        };
        let rest = after[eq + 1..].trim_start();
        let quote = rest.chars().next()?;
        if quote != '\'' && quote != '"' {
            continue;
        }
        let tail = &rest[1..];
        if let Some(end) = tail.find(quote) {
            return Some(tail[..end].to_string());
        }
    }
    None
}

fn assignment_first_word(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (left, right) = line.split_once('=')?;
        (left.trim() == key)
            .then(|| right.split_whitespace().next().map(str::to_string))
            .flatten()
    })
}

fn xml_tag(text: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = text.find(&open)? + open.len();
    let end = text[start..].find(&close)? + start;
    let value = text[start..end].trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn make_target(text: &str) -> Option<String> {
    let mut all_dependency = None;
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') || line.starts_with('.') {
            continue;
        }
        let Some((left, right)) = line.split_once(':') else {
            continue;
        };
        let target = left.trim();
        if target == "all" {
            all_dependency = right
                .split_whitespace()
                .find(|word| plausible_target(word))
                .map(str::to_string);
            continue;
        }
        if plausible_target(target) {
            return Some(target.to_string());
        }
    }
    all_dependency
}

fn plausible_target(value: &str) -> bool {
    !value.is_empty()
        && !value.contains('%')
        && !value.contains('$')
        && !value.contains('=')
        && !value.contains(' ')
        && !matches!(value, "all" | "clean" | "install" | "test" | "check" | "help")
}

fn string_value(value: &str) -> String {
    serde_json::to_string(value).expect("serialize TOML-compatible string")
}

fn string_array(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| string_value(value))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn display_command(command: &[String]) -> String {
    command
        .iter()
        .map(|part| {
            if part.chars().all(|ch| {
                ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | ':' | '=')
            }) {
                part.clone()
            } else {
                format!("{part:?}")
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use tempfile::tempdir;

    #[test]
    fn cargo_detection_uses_manifest_binary_and_lockfile() {
        let dir = tempdir().expect("tempdir");
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"widget-cli\"\nversion = \"0.1.0\"\nrust-version = \"1.86\"\n",
        )
        .expect("write Cargo.toml");
        fs::write(dir.path().join("Cargo.lock"), "# lock\n").expect("write Cargo.lock");

        let plan = detect_project(dir.path());

        assert_eq!(plan.detected_project, "Cargo");
        assert_eq!(plan.confidence, DetectionConfidence::High);
        assert_eq!(plan.image, "rust:1.86");
        assert_eq!(plan.command, vec!["cargo", "build", "--release", "--locked"]);
        assert_eq!(plan.outputs, vec![PathBuf::from("target/release/widget-cli")]);
    }

    #[test]
    fn cmake_detection_extracts_first_executable() {
        let dir = tempdir().expect("tempdir");
        fs::write(
            dir.path().join("CMakeLists.txt"),
            "cmake_minimum_required(VERSION 3.20)\nadd_executable(sample src/main.cpp)\n",
        )
        .expect("write CMakeLists.txt");

        let plan = detect_project(dir.path());

        assert_eq!(plan.detected_project, "CMake");
        assert_eq!(plan.confidence, DetectionConfidence::High);
        assert_eq!(plan.outputs, vec![PathBuf::from("build/sample")]);
    }

    #[test]
    fn pnpm_detection_uses_stable_package_output() {
        let dir = tempdir().expect("tempdir");
        fs::write(
            dir.path().join("package.json"),
            r#"{"name":"demo","version":"1.0.0","scripts":{"build":"tsc"}}"#,
        )
        .expect("write package.json");
        fs::write(dir.path().join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n")
            .expect("write lock");

        let plan = detect_project(dir.path());

        assert_eq!(plan.detected_project, "Node.js / pnpm");
        assert_eq!(plan.outputs, vec![PathBuf::from("build/reprobisect-package.tgz")]);
        assert!(plan.command[2].contains("pnpm run build"));
    }

    #[test]
    fn ambiguity_is_explicit() {
        let dir = tempdir().expect("tempdir");
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        )
        .expect("write Cargo.toml");
        fs::write(dir.path().join("Makefile"), "all:\n\tcargo build\n").expect("write Makefile");

        let plan = detect_project(dir.path());

        assert_eq!(plan.detected_project, "Cargo");
        assert_eq!(plan.confidence, DetectionConfidence::Medium);
        assert!(plan.notes.iter().any(|note| note.contains("multiple build-system markers")));
    }

    #[test]
    fn fallback_config_is_parseable() {
        let rendered = render_config(&generic_plan());
        let parsed: Config = toml::from_str(&rendered).expect("parse generated config");

        assert_eq!(parsed.build.image, "gcc:14");
        assert_eq!(parsed.build.outputs, vec![PathBuf::from("build/app")]);
        assert_eq!(parsed.experiments.control_runs, 2);
    }
}
