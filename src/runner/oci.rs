use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    net::IpAddr,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use filetime::{FileTime, set_file_mtime, set_file_times};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use uuid::Uuid;
use walkdir::WalkDir;

use crate::{
    artifact::collect_artifacts,
    model::{
        BuildFailure, BuildRun, BuildSpec, ControlledEnvironment, DependencyCacheSummary,
        DependencyNetworkCorrelation, NetworkTraceSummary, ProcessActivitySummary, ProcessTraceSummary,
        RuntimeDependencyProvenance, SourceOverrideRecord, ToolchainBindingProbe, ToolchainProbe,
        ToolchainProvenance, RunnerBackend,
    },
    runner::{RunOutcome, Runner},
    schema::BUILD_EVIDENCE_SCHEMA_CURRENT,
    source::{collect_dependency_provenance, copy_source_tree, sha256_file},
};

#[derive(Debug)]
struct CapturedStream {
    text: String,
    sha256: String,
    total_bytes: u64,
    truncated: bool,
}

fn drain_bounded_stream<R: Read>(mut reader: R, max_bytes: u64) -> std::io::Result<CapturedStream> {
    let retain_limit = usize::try_from(max_bytes).unwrap_or(usize::MAX);
    let mut retained = Vec::with_capacity(retain_limit.min(64 * 1024));
    let mut total_bytes = 0_u64;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 16 * 1024];

    loop {
        let read = match reader.read(&mut buffer) {
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if read == 0 {
            break;
        }
        let chunk = &buffer[..read];
        hasher.update(chunk);
        total_bytes = total_bytes.saturating_add(read as u64);
        if retained.len() < retain_limit {
            let remaining = retain_limit - retained.len();
            retained.extend_from_slice(&chunk[..read.min(remaining)]);
        }
    }

    Ok(CapturedStream {
        text: String::from_utf8_lossy(&retained).into_owned(),
        sha256: hex::encode(hasher.finalize()),
        total_bytes,
        truncated: total_bytes > max_bytes,
    })
}

pub struct OciRunner {
    project_root: PathBuf,
    backend: RunnerBackend,
    resolved_image_ids: Mutex<BTreeMap<String, String>>,
    toolchain_cache: Mutex<BTreeMap<String, ToolchainProvenance>>,
}

impl OciRunner {
    pub fn new(project_root: PathBuf, backend: RunnerBackend) -> Self {
        Self {
            project_root,
            backend,
            resolved_image_ids: Mutex::new(BTreeMap::new()),
            toolchain_cache: Mutex::new(BTreeMap::new()),
        }
    }

    fn runtime_executable(&self) -> &'static str {
        self.backend.executable()
    }

    fn prepare_workspace(&self, environment: &ControlledEnvironment) -> Result<TempDir> {
        let temp = tempfile::Builder::new()
            .prefix("reprobisect-run-")
            .tempdir()
            .context("cannot create temporary build workspace")?;
        copy_source_tree(&self.project_root, temp.path(), environment.source_copy_order)?;
        Ok(temp)
    }


    fn apply_source_overrides(
        &self,
        workspace: &Path,
        environment: &ControlledEnvironment,
    ) -> Result<Vec<SourceOverrideRecord>> {
        let mut records = Vec::new();
        for (target, variant_source) in &environment.source_file_overrides {
            let target_path = workspace.join(target);
            let variant_path = workspace.join(variant_source);
            let target_metadata = fs::symlink_metadata(&target_path).with_context(|| {
                format!("cannot stat dependency override target {}", target_path.display())
            })?;
            let variant_metadata = fs::symlink_metadata(&variant_path).with_context(|| {
                format!("cannot stat dependency override source {}", variant_path.display())
            })?;
            if !target_metadata.file_type().is_file() || !variant_metadata.file_type().is_file() {
                bail!(
                    "dependency file overrides require regular files: target={} variant={}",
                    target.display(),
                    variant_source.display()
                );
            }
            let baseline_sha256 = sha256_file(&target_path)?;
            let variant_sha256 = sha256_file(&variant_path)?;
            let target_metadata = fs::metadata(&target_path)
                .with_context(|| format!("cannot read dependency target metadata {}", target_path.display()))?;
            let target_atime = FileTime::from_last_access_time(&target_metadata);
            let target_mtime = FileTime::from_last_modification_time(&target_metadata);
            let variant_bytes = fs::read(&variant_path).with_context(|| {
                format!("cannot read dependency override source {}", variant_path.display())
            })?;
            fs::write(&target_path, variant_bytes).with_context(|| {
                format!(
                    "cannot apply dependency override {} -> {}",
                    variant_source.display(),
                    target.display()
                )
            })?;
            // fs::write truncates the existing target and therefore preserves its
            // permission bits. Restore atime/mtime so the controlled difference is
            // the declaration bytes rather than ordinary file metadata.
            set_file_times(&target_path, target_atime, target_mtime).with_context(|| {
                format!("cannot restore dependency target timestamps {}", target_path.display())
            })?;
            records.push(SourceOverrideRecord {
                target: target.clone(),
                variant_source: variant_source.clone(),
                baseline_sha256,
                variant_sha256,
            });
        }
        records.sort_by(|left, right| left.target.cmp(&right.target));
        Ok(records)
    }

    fn prepare_toolchain_wrappers(
        &self,
        metadata_dir: &Path,
        environment: &ControlledEnvironment,
    ) -> Result<BTreeMap<String, String>> {
        let wrappers_dir = metadata_dir.join("toolchain-wrappers");
        let invocations_dir = metadata_dir.join("toolchain-invocations");
        fs::create_dir_all(&wrappers_dir).context("cannot create toolchain wrapper directory")?;
        fs::create_dir_all(&invocations_dir).context("cannot create toolchain invocation directory")?;

        let mut actuals = String::new();
        let mut runtime_bindings = BTreeMap::new();
        for (variable, configured) in &environment.toolchain_bindings {
            let actual = expand_controlled_value(configured, environment);
            actuals.push_str(variable);
            actuals.push('\t');
            actuals.push_str(&actual);
            actuals.push('\n');

            let wrapper_path = wrappers_dir.join(variable);
            let script = format!(
                "#!/bin/sh\nprintf '.\\n' >> /reprobisect-meta/toolchain-invocations/{variable}.log\nexec {actual} \"$@\"\n"
            );
            fs::write(&wrapper_path, script)
                .with_context(|| format!("cannot write toolchain wrapper {variable}"))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&wrapper_path, fs::Permissions::from_mode(0o755))
                    .with_context(|| format!("cannot mark toolchain wrapper {variable} executable"))?;
            }
            runtime_bindings.insert(
                variable.clone(),
                format!("/reprobisect-meta/toolchain-wrappers/{variable}"),
            );
        }
        fs::write(metadata_dir.join("toolchain-actuals.tsv"), actuals)
            .context("cannot write controlled toolchain binding metadata")?;
        Ok(runtime_bindings)
    }

    fn prepare_dependency_cache_spec(
        &self,
        metadata_dir: &Path,
        environment: &ControlledEnvironment,
    ) -> Result<()> {
        let mut body = String::new();
        for (ecosystem, path) in &environment.dependency_cache_paths {
            body.push_str(ecosystem);
            body.push('\t');
            body.push_str(path);
            body.push('\n');
        }
        fs::write(metadata_dir.join("dependency-cache-paths.tsv"), body)
            .context("cannot write dependency cache provenance specification")
    }

    fn ensure_image(&self, image: &str) -> Result<String> {
        if let Some(id) = self
            .resolved_image_ids
            .lock()
            .map_err(|_| anyhow::anyhow!("OCI image cache lock poisoned"))?
            .get(image)
            .cloned()
        {
            return Ok(id);
        }

        let id = self.resolve_image_uncached(image)?;
        self.resolved_image_ids
            .lock()
            .map_err(|_| anyhow::anyhow!("OCI image cache lock poisoned"))?
            .insert(image.to_string(), id.clone());
        Ok(id)
    }

    fn toolchain_provenance(&self, resolved_image_id: &str) -> ToolchainProvenance {
        if let Ok(cache) = self.toolchain_cache.lock() {
            if let Some(value) = cache.get(resolved_image_id) {
                return value.clone();
            }
        }

        let value = self.probe_toolchain(resolved_image_id);
        if let Ok(mut cache) = self.toolchain_cache.lock() {
            cache.insert(resolved_image_id.to_string(), value.clone());
        }
        value
    }

    fn probe_toolchain(&self, resolved_image_id: &str) -> ToolchainProvenance {
        const SCRIPT: &str = r#"
for tool in cc c++ gcc g++ clang clang++ rustc cargo ld lld ar ranlib python3 python pip pip3 uv poetry node npm pnpm yarn java javac go; do
  if command -v "$tool" >/dev/null 2>&1; then
    version=$("$tool" --version 2>&1 | head -n 1 || true)
    printf '%s\t%s\n' "$tool" "$version"
  fi
done
"#;

        let output = Command::new(self.runtime_executable())
            .args([
                "run",
                "--rm",
                "--network",
                "none",
                "--entrypoint",
                "/bin/sh",
                resolved_image_id,
                "-c",
                SCRIPT,
            ])
            .output();

        let output = match output {
            Ok(output) => output,
            Err(error) => {
                return ToolchainProvenance {
                    probes: Vec::new(),
                    bindings: Vec::new(),
                    error: Some(format!("toolchain probe could not start: {error}")),
                };
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return ToolchainProvenance {
                probes: Vec::new(),
                bindings: Vec::new(),
                error: Some(format!(
                    "toolchain probe unavailable in image: {}",
                    stderr.trim()
                )),
            };
        }

        let mut probes = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| {
                let (tool, version) = line.split_once('\t')?;
                if tool.is_empty() || version.is_empty() {
                    return None;
                }
                Some(ToolchainProbe {
                    tool: tool.to_string(),
                    version: version.to_string(),
                })
            })
            .collect::<Vec<_>>();
        probes.sort_by(|left, right| left.tool.cmp(&right.tool));
        probes.dedup_by(|left, right| left.tool == right.tool);
        ToolchainProvenance { probes, bindings: Vec::new(), error: None }
    }

    fn resolve_image_uncached(&self, image: &str) -> Result<String> {
        let inspect = |image: &str| -> Result<Option<String>> {
            let output = Command::new(self.runtime_executable())
                .args(["image", "inspect", "--format", "{{.Id}}", image])
                .output()
                .with_context(|| {
                    format!(
                        "failed to inspect {} image {image}",
                        self.backend.display_name()
                    )
                })?;
            if output.status.success() {
                let id = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if id.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(id))
                }
            } else {
                Ok(None)
            }
        };

        if let Some(id) = inspect(image)? {
            return Ok(id);
        }

        let pull = Command::new(self.runtime_executable())
            .args(["pull", image])
            .output()
            .with_context(|| {
                format!(
                    "failed to pull {} image {image}",
                    self.backend.display_name()
                )
            })?;
        if !pull.status.success() {
            bail!(
                "failed to pull {} image {image}: {}",
                self.backend.display_name(),
                String::from_utf8_lossy(&pull.stderr).trim()
            );
        }

        inspect(image)?.ok_or_else(|| {
            anyhow::anyhow!(
                "{} image {image} is still unavailable after pull",
                self.backend.display_name()
            )
        })
    }

    fn runtime_args(
        &self,
        spec: &BuildSpec,
        workspace: &Path,
        metadata_dir: &Path,
        environment: &ControlledEnvironment,
        effective_environment: &BTreeMap<String, String>,
        resolved_image_id: &str,
        container_name: &str,
        command: &[String],
    ) -> Result<Vec<String>> {
        let host_workspace = fs::canonicalize(workspace)
            .with_context(|| format!("cannot canonicalize {}", workspace.display()))?;
        let host_mount = host_workspace
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("workspace path is not valid UTF-8"))?;
        let host_metadata = fs::canonicalize(metadata_dir)
            .with_context(|| format!("cannot canonicalize {}", metadata_dir.display()))?;
        let host_metadata_mount = host_metadata
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("metadata path is not valid UTF-8"))?;

        let working_dir = if spec.working_directory == PathBuf::from(".") {
            environment.container_work_path.clone()
        } else {
            format!(
                "{}/{}",
                environment.container_work_path.trim_end_matches('/'),
                spec.working_directory.to_string_lossy()
            )
        };

        let mut args = vec![
            "run".to_string(),
            "--rm".to_string(),
            "--name".to_string(),
            container_name.to_string(),
            "--mount".to_string(),
            format!("type=bind,src={host_mount},dst={}", environment.container_source_path),
            "--mount".to_string(),
            format!("type=bind,src={host_metadata_mount},dst=/reprobisect-meta"),
        ];

        if environment.container_work_path != environment.container_source_path {
            args.push("--mount".to_string());
            args.push(format!(
                "type=bind,src={host_mount},dst={}",
                environment.container_work_path
            ));
        }

        args.push("--workdir".to_string());
        args.push(working_dir);

        if let Some(hostname) = &environment.hostname {
            args.push("--hostname".to_string());
            args.push(hostname.clone());
        }

        if let Some(cpu_count) = environment.cpu_count {
            args.push("--cpus".to_string());
            args.push(cpu_count.to_string());
        }

        if environment.network_mode != "default" {
            args.push("--network".to_string());
            args.push(environment.network_mode.clone());
        }

        if environment.network_trace || environment.file_input_trace {
            // Tracing is explicitly opt-in. SYS_PTRACE is scoped to this build
            // container/PID namespace and is still followed by an in-container
            // strace preflight before instrumentation is trusted.
            args.push("--cap-add".to_string());
            args.push("SYS_PTRACE".to_string());
        }

        for (key, value) in effective_environment {
            args.push("--env".to_string());
            args.push(format!("{key}={value}"));
        }

        args.push(resolved_image_id.to_string());

        let needs_wrapper = environment.umask.is_some()
            || !environment.toolchain_bindings.is_empty()
            || environment.network_trace
            || environment.file_input_trace
            || environment.runtime_dependency_provenance;
        if needs_wrapper {
            const WRAPPER: &str = r#"
umask_value="$1"
toolchain_probe="$2"
toolchain_spec="$3"
trace_status="$4"
syscall_trace="$5"
network_trace_enabled="$6"
file_trace_enabled="$7"
dependency_status="$8"
dependency_before="$9"
dependency_after="${10}"
dependency_spec="${11}"
dependency_enabled="${12}"
dependency_max_files="${13}"
dependency_max_bytes="${14}"
shift 14
if [ -n "$umask_value" ]; then
  umask "$umask_value"
fi
: > "$toolchain_probe"
if [ -f "$toolchain_spec" ]; then
  tab=$(printf '\t')
  while IFS="$tab" read -r key tool; do
    [ -n "$key" ] || continue
    resolved=$(command -v "$tool" 2>/dev/null || true)
    if [ -n "$resolved" ]; then
      version=$("$tool" --version 2>&1 | head -n 1 | tr '\t' ' ' || true)
      printf '%s\t%s\t%s\t%s\n' "$key" "$tool" "$resolved" "$version" >> "$toolchain_probe"
    else
      printf '%s\t%s\t\t\n' "$key" "$tool" >> "$toolchain_probe"
    fi
  done < "$toolchain_spec"
fi

dependency_tools_available=0
if [ "$dependency_enabled" = "1" ]; then
  : > "$dependency_before"
  : > "$dependency_after"
  if command -v find >/dev/null 2>&1 && command -v sha256sum >/dev/null 2>&1 \
     && command -v sort >/dev/null 2>&1 && command -v awk >/dev/null 2>&1 \
     && command -v wc >/dev/null 2>&1 && command -v tr >/dev/null 2>&1 \
     && command -v stat >/dev/null 2>&1; then
    dependency_tools_available=1
    printf 'available\n' > "$dependency_status"
    summarize_cache_to() {
      output="$1"
      ecosystem="$2"
      directory="$3"
      [ -d "$directory" ] || return 0
      tmp="/reprobisect-meta/cache-summary-${ecosystem}-$$"
      state="/reprobisect-meta/cache-state-${ecosystem}-$$"
      truncated="/reprobisect-meta/cache-truncated-${ecosystem}-$$"
      : > "$tmp"
      printf '0\t0\n' > "$state"
      rm -f "$truncated"
      find "$directory" -type f -exec sh -c '
        out="$1"; state="$2"; truncated="$3"; max_files="$4"; max_bytes="$5"; shift 5
        [ -f "$truncated" ] && exit 0
        tab=$(printf "\\t")
        IFS="$tab" read -r count bytes < "$state"
        count=${count:-0}
        bytes=${bytes:-0}
        for file do
          if [ "$count" -ge "$max_files" ]; then
            : > "$truncated"
            break
          fi
          size=$(stat -c %s "$file" 2>/dev/null || true)
          case "$size" in
            ""|*[!0-9]*) : > "$truncated"; break ;;
          esac
          remaining=$((max_bytes - bytes))
          if [ "$size" -gt "$remaining" ]; then
            : > "$truncated"
            break
          fi
          digest=$(sha256sum "$file" 2>/dev/null || true)
          digest=${digest%% *}
          if [ -z "$digest" ]; then
            : > "$truncated"
            break
          fi
          printf "%s\\t%s\\n" "$digest" "$size" >> "$out"
          count=$((count + 1))
          bytes=$((bytes + size))
        done
        printf "%s\\t%s\\n" "$count" "$bytes" > "$state"
      ' sh "$tmp" "$state" "$truncated" "$dependency_max_files" "$dependency_max_bytes" {} + 2>/dev/null || : > "$truncated"
      tab=$(printf '\t')
      IFS="$tab" read -r count bytes < "$state"
      aggregate=""
      if [ "${count:-0}" -gt 0 ]; then
        aggregate=$(awk -F '\t' '{print $1}' "$tmp" | sort | sha256sum | awk '{print $1}')
      fi
      truncated_value=0
      [ -f "$truncated" ] && truncated_value=1
      rm -f "$tmp" "$state" "$truncated"
      printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$ecosystem" "${count:-0}" "${bytes:-0}" "$aggregate" "$truncated_value" \
        "$dependency_max_files" "$dependency_max_bytes" >> "$output"
    }
    summarize_all_caches() {
      output="$1"
      home_dir="${HOME:-/root}"
      summarize_cache_to "$output" cargo "${CARGO_HOME:-$home_dir/.cargo}/registry/cache"
      summarize_cache_to "$output" npm "${npm_config_cache:-$home_dir/.npm}/_cacache/content-v2"
      summarize_cache_to "$output" pip "${PIP_CACHE_DIR:-$home_dir/.cache/pip}"
      summarize_cache_to "$output" uv "$home_dir/.cache/uv"
      summarize_cache_to "$output" poetry "$home_dir/.cache/pypoetry"
      summarize_cache_to "$output" go "$home_dir/go/pkg/mod/cache/download"
      summarize_cache_to "$output" pnpm "$home_dir/.local/share/pnpm/store"
      summarize_cache_to "$output" yarn "$home_dir/.cache/yarn"
      if [ -f "$dependency_spec" ]; then
        tab=$(printf '\t')
        while IFS="$tab" read -r ecosystem directory; do
          [ -n "$ecosystem" ] || continue
          summarize_cache_to "$output" "$ecosystem" "$directory"
        done < "$dependency_spec"
      fi
    }
    summarize_all_caches "$dependency_before"
  else
    printf 'unavailable\n' > "$dependency_status"
  fi
fi

trace_requested=0
if [ "$network_trace_enabled" = "1" ] || [ "$file_trace_enabled" = "1" ]; then
  trace_requested=1
fi
if [ "$trace_requested" = "1" ]; then
  trace_classes="process"
  [ "$network_trace_enabled" = "1" ] && trace_classes="$trace_classes,network"
  # File-input tracing includes mmap plus the descriptor lifecycle primitives
  # required to correlate a readable mapping with the file descriptor that
  # produced it. These remain opt-in because descriptor/memory tracing can be
  # materially noisier than network-only observation.
  [ "$file_trace_enabled" = "1" ] && trace_classes="$trace_classes,file,mmap,close,dup,dup2,dup3,fcntl"
  if command -v strace >/dev/null 2>&1 \
     && strace -qq -e "trace=$trace_classes" -o /reprobisect-meta/strace-preflight.trace -- /bin/sh -c ':' >/dev/null 2>&1; then
    printf 'available\n' > "$trace_status"
    strace -f -qq -s 256 -e "trace=$trace_classes" -o "$syscall_trace" -- "$@"
    command_status=$?
  else
    printf 'unavailable\n' > "$trace_status"
    "$@"
    command_status=$?
  fi
else
  "$@"
  command_status=$?
fi

if [ "$dependency_enabled" = "1" ] && [ "$dependency_tools_available" = "1" ]; then
  summarize_all_caches "$dependency_after"
fi
exit "$command_status"
"#;
            let toolchain_probe = "/reprobisect-meta/toolchain-bindings.tsv".to_string();
            let toolchain_spec = "/reprobisect-meta/toolchain-actuals.tsv".to_string();
            let trace_status = "/reprobisect-meta/trace-status".to_string();
            let syscall_trace = "/reprobisect-meta/syscall.trace".to_string();
            let dependency_status = "/reprobisect-meta/dependency-status".to_string();
            let dependency_before = "/reprobisect-meta/dependency-cache-before.tsv".to_string();
            let dependency_after = "/reprobisect-meta/dependency-cache-after.tsv".to_string();
            let dependency_spec = "/reprobisect-meta/dependency-cache-paths.tsv".to_string();
            args.extend([
                "/bin/sh".to_string(),
                "-c".to_string(),
                WRAPPER.to_string(),
                "reprobisect".to_string(),
                environment
                    .umask
                    .map(|value| format!("{value:03o}"))
                    .unwrap_or_default(),
                toolchain_probe,
                toolchain_spec,
                trace_status,
                syscall_trace,
                if environment.network_trace { "1" } else { "0" }.to_string(),
                if environment.file_input_trace { "1" } else { "0" }.to_string(),
                dependency_status,
                dependency_before,
                dependency_after,
                dependency_spec,
                if environment.runtime_dependency_provenance { "1" } else { "0" }.to_string(),
                environment.dependency_cache_max_files.to_string(),
                environment.dependency_cache_max_bytes.to_string(),
            ]);
        }
        args.extend(command.iter().cloned());
        Ok(args)
    }
}

fn expand_command(command: &[String], environment: &ControlledEnvironment) -> Vec<String> {
    command
        .iter()
        .map(|argument| {
            argument
                .replace("{source}", &environment.container_source_path)
                .replace("{build}", &environment.container_work_path)
        })
        .collect()
}

fn expand_controlled_value(value: &str, environment: &ControlledEnvironment) -> String {
    value
        .replace("{source}", &environment.container_source_path)
        .replace("{build}", &environment.container_work_path)
}

fn parse_toolchain_binding_probes(path: &Path, invocations_dir: &Path) -> Vec<ToolchainBindingProbe> {
    let Ok(raw) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut probes = raw
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(4, '\t');
            let variable = parts.next()?.to_string();
            let configured_value = parts.next()?.to_string();
            let resolved_path = parts
                .next()
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            let version = parts
                .next()
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            let error = resolved_path
                .is_none()
                .then(|| "configured executable was not resolvable inside the build container".to_string());
            let invocation_count = fs::read_to_string(invocations_dir.join(format!("{variable}.log")))
                .map(|raw| raw.lines().count())
                .unwrap_or(0);
            Some(ToolchainBindingProbe {
                variable,
                configured_value,
                resolved_path,
                version,
                invocation_count,
                error,
            })
        })
        .collect::<Vec<_>>();
    probes.sort_by(|left, right| left.variable.cmp(&right.variable));
    probes
}

fn extract_quoted_after<'a>(line: &'a str, marker: &str) -> Option<&'a str> {
    let start = line.find(marker)? + marker.len();
    let rest = &line[start..];
    let end = rest.find('"')?;
    Some(&rest[..end])
}

fn classify_ip_scope(raw: &str) -> &'static str {
    let Ok(ip) = raw.parse::<IpAddr>() else {
        return "unknown";
    };
    if ip.is_loopback() {
        return "loopback";
    }
    match ip {
        IpAddr::V4(ip) if ip.is_private() => "private",
        IpAddr::V4(ip) if ip.is_link_local() => "link_local",
        IpAddr::V4(ip) if ip.is_unspecified() => "unspecified",
        IpAddr::V6(ip) if ip.is_unique_local() => "private",
        IpAddr::V6(ip) if ip.is_unicast_link_local() => "link_local",
        IpAddr::V6(ip) if ip.is_unspecified() => "unspecified",
        _ => "public",
    }
}

fn network_endpoint_scope(line: &str) -> Option<&'static str> {
    if line.contains("AF_UNIX") {
        return Some("unix");
    }
    if line.contains("AF_NETLINK") {
        return Some("kernel");
    }
    if let Some(ip) = extract_quoted_after(line, "inet_addr(\"") {
        return Some(classify_ip_scope(ip));
    }
    if let Some(ip) = extract_quoted_after(line, "inet_pton(AF_INET6, \"") {
        return Some(classify_ip_scope(ip));
    }
    if line.contains("AF_INET6") || line.contains("AF_INET") {
        return Some("unknown");
    }
    None
}

fn network_syscall_succeeded(line: &str) -> bool {
    syscall_result_i64(line).is_some_and(|value| value >= 0)
}

fn syscall_result_i64(line: &str) -> Option<i64> {
    let (_, result) = line.rsplit_once(" = ")?;
    result.trim().split_whitespace().next()?.parse::<i64>().ok()
}

fn syscall_succeeded(line: &str) -> bool {
    let Some((_, result)) = line.rsplit_once(" = ") else {
        return false;
    };
    let token = result.trim().split_whitespace().next().unwrap_or("");
    if let Ok(value) = token.parse::<i64>() {
        return value >= 0;
    }
    token
        .strip_prefix("0x")
        .and_then(|hex| u64::from_str_radix(hex, 16).ok())
        .is_some()
}

fn quoted_arguments(line: &str) -> Vec<&str> {
    let mut values = Vec::new();
    let mut start = None;
    let mut escaped = false;
    for (index, ch) in line.char_indices() {
        if let Some(begin) = start {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                values.push(&line[begin..index]);
                start = None;
            }
        } else if ch == '"' {
            start = Some(index + 1);
        }
    }
    values
}

fn syscall_arguments<'a>(body: &'a str, name: &str) -> Option<Vec<&'a str>> {
    let marker = format!("{name}(");
    let start = body.find(&marker)? + marker.len();
    let rest = &body[start..];
    let end = rest.rfind(") = ")?;
    let raw = &rest[..end];
    Some(raw.split(',').map(str::trim).collect())
}

fn process_child_pid(body: &str) -> Option<String> {
    let is_creation = [
        "clone(",
        "clone3(",
        "fork(",
        "vfork(",
        "clone resumed",
        "clone3 resumed",
        "fork resumed",
        "vfork resumed",
    ]
        .iter()
        .any(|marker| body.contains(marker));
    if !is_creation {
        return None;
    }
    let child = syscall_result_i64(body)?;
    (child > 0).then(|| child.to_string())
}

fn mmap_fd(body: &str) -> Option<i64> {
    let args = syscall_arguments(body, "mmap")?;
    if args.len() < 6 || !args[2].contains("PROT_READ") || args[3].contains("MAP_ANONYMOUS") {
        return None;
    }
    args[4].parse::<i64>().ok().filter(|fd| *fd >= 0)
}

fn close_fd(body: &str) -> Option<i64> {
    if syscall_result_i64(body) != Some(0) {
        return None;
    }
    syscall_arguments(body, "close")?
        .first()?
        .parse::<i64>()
        .ok()
}

fn duplicated_fd(body: &str) -> Option<(i64, i64, bool)> {
    for name in ["dup", "dup2", "dup3"] {
        let Some(args) = syscall_arguments(body, name) else {
            continue;
        };
        let new_fd = syscall_result_i64(body)?;
        if new_fd < 0 {
            return None;
        }
        let old_fd = args.first()?.parse::<i64>().ok()?;
        let cloexec = name == "dup3" && args.iter().any(|arg| arg.contains("O_CLOEXEC"));
        return Some((old_fd, new_fd, cloexec));
    }
    None
}

fn fcntl_descriptor_update(body: &str) -> Option<(i64, Option<i64>, Option<bool>)> {
    let args = syscall_arguments(body, "fcntl")?;
    let fd = args.first()?.parse::<i64>().ok()?;
    let command = *args.get(1)?;
    if command == "F_SETFD" {
        if syscall_result_i64(body) != Some(0) {
            return None;
        }
        let cloexec = args
            .get(2)
            .is_some_and(|flags| flags.contains("FD_CLOEXEC"));
        return Some((fd, None, Some(cloexec)));
    }
    if matches!(command, "F_DUPFD" | "F_DUPFD_CLOEXEC") {
        let new_fd = syscall_result_i64(body)?;
        if new_fd < 0 {
            return None;
        }
        return Some((
            fd,
            Some(new_fd),
            Some(command == "F_DUPFD_CLOEXEC"),
        ));
    }
    None
}

fn trace_status(metadata_dir: &Path) -> (bool, Option<String>) {
    let status = fs::read_to_string(metadata_dir.join("trace-status")).unwrap_or_default();
    match status.trim() {
        "available" => (true, None),
        "unavailable" => (
            false,
            Some(
                "strace was unavailable or blocked by the container runtime; syscall provenance was not collected"
                    .to_string(),
            ),
        ),
        _ => (
            false,
            Some("syscall trace status was not produced by the build wrapper".to_string()),
        ),
    }
}

fn read_bounded_trace(metadata_dir: &Path, max_bytes: u64) -> Result<(String, u64, bool)> {
    let path = metadata_dir.join("syscall.trace");
    let file = fs::File::open(&path)
        .with_context(|| format!("cannot read syscall trace {}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .context("cannot read bounded syscall trace")?;
    let truncated = bytes.len() as u64 > max_bytes;
    if truncated {
        bytes.truncate(max_bytes as usize);
    }
    let parsed_bytes = bytes.len() as u64;
    Ok((String::from_utf8_lossy(&bytes).into_owned(), parsed_bytes, truncated))
}

fn trace_pid_and_body(line: &str) -> (&str, &str) {
    let trimmed = line.trim_start();
    if let Some(rest) = trimmed.strip_prefix("[pid ") {
        if let Some(end) = rest.find(']') {
            let pid = rest[..end].trim();
            return (pid, rest[end + 1..].trim_start());
        }
    }
    let digits = trimmed.chars().take_while(|ch| ch.is_ascii_digit()).count();
    if digits > 0 && trimmed[digits..].chars().next().is_some_and(char::is_whitespace) {
        return (&trimmed[..digits], trimmed[digits..].trim_start());
    }
    ("root", trimmed)
}

fn first_quoted_argument(line: &str) -> Option<&str> {
    let start = line.find('"')? + 1;
    let rest = &line[start..];
    let mut escaped = false;
    for (index, ch) in rest.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return Some(&rest[..index]);
        }
    }
    None
}

fn process_role_from_exec(path: &str) -> &'static str {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name {
        "cc" | "c++" | "gcc" | "g++" | "clang" | "clang++" | "rustc" => "compiler",
        "ld" | "ld.lld" | "lld" | "lld-link" => "linker",
        "ar" | "ranlib" => "archiver",
        "cargo" | "npm" | "npx" | "pnpm" | "yarn" | "pip" | "pip3" | "uv"
        | "poetry" | "go" | "bundle" | "bundler" | "composer" | "gradle" | "gradlew" => {
            "package_manager"
        }
        "make" | "gmake" | "ninja" | "cmake" | "meson" => "build_tool",
        "tar" | "zip" | "gzip" | "xz" | "zstd" => "packager",
        "python" | "python3" | "ruby" | "perl" => "interpreter",
        "sh" | "bash" | "dash" | "zsh" => "shell",
        _ => "other",
    }
}

fn update_process_role<'a>(body: &'a str, pid: &str, roles: &mut BTreeMap<String, String>) {
    if body.contains("execve(") {
        if let Some(path) = first_quoted_argument(body) {
            roles.insert(pid.to_string(), process_role_from_exec(path).to_string());
        }
    }
}

fn is_nonlocal_scope(scope: &str) -> bool {
    matches!(scope, "public" | "private" | "link_local")
}

fn parse_network_trace(
    metadata_dir: &Path,
    attempted: bool,
    max_bytes: u64,
) -> NetworkTraceSummary {
    if !attempted {
        return NetworkTraceSummary::default();
    }
    let (tracer_available, status_error) = trace_status(metadata_dir);
    if !tracer_available {
        return NetworkTraceSummary {
            attempted: true,
            tracer_available: false,
            error: status_error,
            ..NetworkTraceSummary::default()
        };
    }

    let (raw, parsed_bytes, trace_truncated) = match read_bounded_trace(metadata_dir, max_bytes) {
        Ok(value) => value,
        Err(error) => {
            return NetworkTraceSummary {
                attempted: true,
                tracer_available: true,
                error: Some(format!("syscall trace could not be read: {error}")),
                ..NetworkTraceSummary::default()
            };
        }
    };

    let mut socket_calls = 0;
    let mut connect_calls = 0;
    let mut successful_connects = 0;
    let mut failed_connects = 0;
    let mut sendto_calls = 0;
    let mut recvfrom_calls = 0;
    let mut endpoint_scopes = BTreeMap::new();
    let mut successful_endpoint_scopes = BTreeMap::new();
    let mut successful_nonlocal_by_role = BTreeMap::new();
    let mut roles: BTreeMap<String, String> = BTreeMap::new();
    let mut address_families = std::collections::BTreeSet::new();

    for line in raw.lines() {
        let (pid, body) = trace_pid_and_body(line);
        update_process_role(body, pid, &mut roles);
        for family in ["AF_INET", "AF_INET6", "AF_UNIX", "AF_NETLINK"] {
            if body.contains(family) {
                address_families.insert(family.to_string());
            }
        }
        if body.contains("socket(") {
            socket_calls += 1;
        }
        let is_connect = body.contains("connect(");
        let is_sendto = body.contains("sendto(");
        if is_connect {
            connect_calls += 1;
            if network_syscall_succeeded(body) {
                successful_connects += 1;
            } else {
                failed_connects += 1;
            }
        }
        if is_sendto {
            sendto_calls += 1;
        }
        if body.contains("recvfrom(") {
            recvfrom_calls += 1;
        }
        if is_connect || is_sendto {
            if let Some(scope) = network_endpoint_scope(body) {
                *endpoint_scopes.entry(scope.to_string()).or_insert(0) += 1;
                if network_syscall_succeeded(body) {
                    *successful_endpoint_scopes.entry(scope.to_string()).or_insert(0) += 1;
                    if is_nonlocal_scope(scope) {
                        let role = roles.get(pid).map(String::as_str).unwrap_or("unknown");
                        *successful_nonlocal_by_role.entry(role.to_string()).or_insert(0) += 1;
                    }
                }
            }
        }
    }

    NetworkTraceSummary {
        attempted: true,
        tracer_available: true,
        socket_calls,
        connect_calls,
        successful_connects,
        failed_connects,
        sendto_calls,
        recvfrom_calls,
        address_families: address_families.into_iter().collect(),
        endpoint_scopes,
        successful_endpoint_scopes,
        successful_nonlocal_by_role,
        trace_truncated,
        parsed_bytes,
        error: None,
    }
}

fn dependency_trace_path(path: &str) -> bool {
    const NAMES: &[&str] = &[
        "Cargo.lock", "package-lock.json", "npm-shrinkwrap.json", "pnpm-lock.yaml",
        "yarn.lock", "bun.lock", "bun.lockb", "poetry.lock", "Pipfile.lock", "uv.lock",
        "requirements.txt", "requirements.lock", "go.sum", "Gemfile.lock", "composer.lock",
        "mix.lock", "gradle.lockfile",
    ];
    NAMES.iter().any(|name| path == *name || path.ends_with(&format!("/{name}")))
        || path.ends_with("/vendor/modules.txt")
}

fn cache_trace_path(path: &str, environment: &ControlledEnvironment) -> bool {
    const BUILTIN: &[&str] = &[
        "/.cargo/registry/", "/.npm/", "/.cache/pip/", "/.cache/uv/",
        "/.cache/pypoetry/", "/go/pkg/mod/", "/.local/share/pnpm/", "/.cache/yarn/",
    ];
    BUILTIN.iter().any(|fragment| path.contains(fragment))
        || environment
            .dependency_cache_paths
            .values()
            .any(|root| path == root || path.starts_with(&format!("{}/", root.trim_end_matches('/'))))
}

fn output_trace_path(path: &str, outputs: &[PathBuf], environment: &ControlledEnvironment) -> bool {
    outputs.iter().any(|output| {
        let relative = output.to_string_lossy().replace('\\', "/");
        let full = format!("{}/{}", environment.container_work_path.trim_end_matches('/'), relative);
        path == relative || path == full
    })
}

fn file_open_path(body: &str) -> Option<&str> {
    if body.contains("open(") || body.contains("openat(") || body.contains("openat2(") || body.contains("creat(") {
        first_quoted_argument(body)
    } else {
        None
    }
}

fn file_open_writes(body: &str) -> bool {
    body.contains("creat(")
        || body.contains("O_WRONLY")
        || body.contains("O_RDWR")
        || body.contains("O_CREAT")
        || body.contains("O_TRUNC")
        || body.contains("O_APPEND")
}

#[derive(Debug, Clone, Copy, Default)]
struct TraceFdClass {
    dependency: bool,
    cache: bool,
    cloexec: bool,
}

#[derive(Clone, Default)]
struct TraceActivity {
    role: String,
    dependency_reads: usize,
    dependency_mmaps: usize,
    cache_reads: usize,
    cache_mmaps: usize,
    output_writes: usize,
    output_publications: usize,
    temp_output_publications: usize,
    lineage_temp_output_publications: usize,
    successful_nonlocal_network_events: usize,
}

impl TraceActivity {
    fn has_dependency(&self) -> bool {
        self.dependency_reads > 0 || self.dependency_mmaps > 0
    }

    fn has_cache(&self) -> bool {
        self.cache_reads > 0 || self.cache_mmaps > 0
    }

    fn has_output(&self) -> bool {
        self.output_writes > 0 || self.output_publications > 0
    }

    fn has_network(&self) -> bool {
        self.successful_nonlocal_network_events > 0
    }

    fn is_active(&self) -> bool {
        self.has_dependency() || self.has_cache() || self.has_output() || self.has_network()
    }
}

fn is_ancestor(
    possible_ancestor: &str,
    process: &str,
    parents: &BTreeMap<String, String>,
) -> bool {
    let mut current = process;
    let mut seen = BTreeSet::new();
    while let Some(parent) = parents.get(current) {
        if !seen.insert(current.to_string()) {
            break;
        }
        if parent == possible_ancestor {
            return true;
        }
        current = parent;
    }
    false
}

fn lineage_depth(process: &str, parents: &BTreeMap<String, String>) -> usize {
    let mut current = process;
    let mut depth = 0;
    let mut seen = BTreeSet::new();
    while let Some(parent) = parents.get(current) {
        if !seen.insert(current.to_string()) {
            break;
        }
        depth += 1;
        current = parent;
    }
    depth
}

fn parse_process_trace(
    metadata_dir: &Path,
    attempted: bool,
    network_enabled: bool,
    max_bytes: u64,
    outputs: &[PathBuf],
    environment: &ControlledEnvironment,
) -> ProcessTraceSummary {
    if !attempted {
        return ProcessTraceSummary::default();
    }
    let (tracer_available, status_error) = trace_status(metadata_dir);
    if !tracer_available {
        return ProcessTraceSummary {
            attempted: true,
            tracer_available: false,
            error: status_error,
            ..ProcessTraceSummary::default()
        };
    }
    let (raw, parsed_bytes, truncated) = match read_bounded_trace(metadata_dir, max_bytes) {
        Ok(value) => value,
        Err(error) => {
            return ProcessTraceSummary {
                attempted: true,
                tracer_available: true,
                error: Some(format!("syscall trace could not be read: {error}")),
                ..ProcessTraceSummary::default()
            };
        }
    };

    let mut roles: BTreeMap<String, String> = BTreeMap::new();
    let mut activity: BTreeMap<String, TraceActivity> = BTreeMap::new();
    let mut parents: BTreeMap<String, String> = BTreeMap::new();
    // Collect process-creation edges before the activity pass. strace may emit
    // an <unfinished ...>/<... resumed> pair around clone/fork while the child
    // has already started producing trace lines; the pre-pass lets those child
    // lines still inherit coarse parent context.
    for line in raw.lines() {
        let (pid, body) = trace_pid_and_body(line);
        if let Some(child) = process_child_pid(body) {
            parents.entry(child).or_insert_with(|| pid.to_string());
        }
    }
    let mut descriptor_classes: BTreeMap<String, BTreeMap<i64, TraceFdClass>> = BTreeMap::new();
    // Raw paths exist only while the bounded trace is parsed. They are used to
    // correlate a temporary file with a later rename into a declared output,
    // then discarded before ProcessTraceSummary is constructed.
    let mut path_writers: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for line in raw.lines() {
        let (pid, body) = trace_pid_and_body(line);
        if let Some(parent) = parents.get(pid) {
            if !roles.contains_key(pid) {
                if let Some(parent_role) = roles.get(parent).cloned() {
                    roles.insert(pid.to_string(), parent_role);
                }
            }
            if !descriptor_classes.contains_key(pid) {
                if let Some(parent_fds) = descriptor_classes.get(parent).cloned() {
                    descriptor_classes.insert(pid.to_string(), parent_fds);
                }
            }
        }
        // A successful exec closes descriptors carrying FD_CLOEXEC. Model
        // this before role reassignment so inherited input evidence cannot be
        // fabricated across an exec boundary.
        if body.contains("execve(") && syscall_succeeded(body) {
            if let Some(process_fds) = descriptor_classes.get_mut(pid) {
                process_fds.retain(|_, class| !class.cloexec);
            }
        }
        update_process_role(body, pid, &mut roles);
        let role = roles.get(pid).cloned().unwrap_or_else(|| "unknown".to_string());

        if let Some(child) = process_child_pid(body) {
            if let Some(parent_role) = roles.get(pid).cloned() {
                roles.entry(child.clone()).or_insert(parent_role);
            }
            if let Some(parent_fds) = descriptor_classes.get(pid).cloned() {
                descriptor_classes.entry(child).or_insert(parent_fds);
            }
        }

        if let Some(fd) = close_fd(body) {
            if let Some(process_fds) = descriptor_classes.get_mut(pid) {
                process_fds.remove(&fd);
            }
        }
        if let Some((old_fd, new_fd, cloexec)) = duplicated_fd(body) {
            let class = descriptor_classes
                .get(pid)
                .and_then(|fds| fds.get(&old_fd))
                .copied();
            let process_fds = descriptor_classes.entry(pid.to_string()).or_default();
            if let Some(mut class) = class {
                // POSIX dup/dup2 clear close-on-exec on the new descriptor;
                // dup3 may request it explicitly.
                class.cloexec = cloexec;
                process_fds.insert(new_fd, class);
            } else {
                process_fds.remove(&new_fd);
            }
        }
        if let Some((old_fd, duplicate_fd, cloexec)) = fcntl_descriptor_update(body) {
            if let Some(new_fd) = duplicate_fd {
                let class = descriptor_classes
                    .get(pid)
                    .and_then(|fds| fds.get(&old_fd))
                    .copied();
                let process_fds = descriptor_classes.entry(pid.to_string()).or_default();
                if let Some(mut class) = class {
                    class.cloexec = cloexec.unwrap_or(false);
                    process_fds.insert(new_fd, class);
                } else {
                    process_fds.remove(&new_fd);
                }
            } else if let Some(cloexec) = cloexec {
                if let Some(class) = descriptor_classes
                    .get_mut(pid)
                    .and_then(|fds| fds.get_mut(&old_fd))
                {
                    class.cloexec = cloexec;
                }
            }
        }

        if let Some(path) = file_open_path(body) {
            if network_syscall_succeeded(body) {
                let writes = file_open_writes(body);
                let entry = activity.entry(pid.to_string()).or_default();
                entry.role = role.clone();
                let dependency = !writes && dependency_trace_path(path);
                let cache = !writes && cache_trace_path(path, environment);
                let output = writes && output_trace_path(path, outputs, environment);
                if dependency {
                    entry.dependency_reads += 1;
                }
                if cache {
                    entry.cache_reads += 1;
                }
                if output {
                    entry.output_writes += 1;
                }
                if writes && !output {
                    path_writers
                        .entry(path.to_string())
                        .or_default()
                        .insert(pid.to_string());
                }

                if let Some(fd) = syscall_result_i64(body).filter(|value| *value >= 0) {
                    let process_fds = descriptor_classes.entry(pid.to_string()).or_default();
                    if dependency || cache {
                        process_fds.insert(
                            fd,
                            TraceFdClass {
                                dependency,
                                cache,
                                cloexec: body.contains("O_CLOEXEC"),
                            },
                        );
                    } else {
                        // A newly opened unrelated descriptor can reuse an old
                        // numeric fd; clear any stale classification.
                        process_fds.remove(&fd);
                    }
                }
            }
        }

        if syscall_succeeded(body) {
            if let Some(fd) = mmap_fd(body) {
                if let Some(class) = descriptor_classes
                    .get(pid)
                    .and_then(|fds| fds.get(&fd))
                    .copied()
                {
                    let entry = activity.entry(pid.to_string()).or_default();
                    entry.role = role.clone();
                    if class.dependency {
                        entry.dependency_mmaps += 1;
                    }
                    if class.cache {
                        entry.cache_mmaps += 1;
                    }
                }
            }

            if body.contains("rename(") || body.contains("renameat(") || body.contains("renameat2(") {
                let paths = quoted_arguments(body);
                if paths.len() >= 2 {
                    let source = paths[0];
                    let destination = paths[1];
                    let writers = path_writers.get(source).cloned().unwrap_or_default();
                    if output_trace_path(destination, outputs, environment) {
                        let same_process = writers.contains(pid);
                        let lineage_related = writers.iter().any(|writer| {
                            writer != pid
                                && (is_ancestor(writer, pid, &parents)
                                    || is_ancestor(pid, writer, &parents))
                        });
                        let entry = activity.entry(pid.to_string()).or_default();
                        entry.role = role.clone();
                        entry.output_publications += 1;
                        if same_process {
                            entry.temp_output_publications += 1;
                        }
                        if lineage_related {
                            entry.lineage_temp_output_publications += 1;
                        }
                    }

                    // Follow temporary-file renames transitively without ever
                    // retaining the path after this parser returns.
                    if let Some(source_writers) = path_writers.remove(source) {
                        path_writers
                            .entry(destination.to_string())
                            .or_default()
                            .extend(source_writers);
                    }
                }
            }
        }

        if network_enabled && (body.contains("connect(") || body.contains("sendto("))
            && network_syscall_succeeded(body)
        {
            if let Some(scope) = network_endpoint_scope(body) {
                if is_nonlocal_scope(scope) {
                    let entry = activity.entry(pid.to_string()).or_default();
                    entry.role = role;
                    entry.successful_nonlocal_network_events += 1;
                }
            }
        }
    }

    // Retain active processes plus their traced ancestors so parent references
    // are meaningful without exposing raw process identifiers.
    let active_pids = activity
        .iter()
        .filter(|(_, item)| item.is_active())
        .map(|(pid, _)| pid.clone())
        .collect::<BTreeSet<_>>();
    let mut relevant_pids = active_pids.clone();
    for process in &active_pids {
        let mut current = process;
        let mut seen = BTreeSet::new();
        while let Some(parent) = parents.get(current) {
            if !seen.insert(current.clone()) {
                break;
            }
            relevant_pids.insert(parent.clone());
            current = parent;
        }
    }
    let anonymous = relevant_pids
        .iter()
        .enumerate()
        .map(|(index, pid)| (pid.clone(), format!("p{}", index + 1)))
        .collect::<BTreeMap<_, _>>();

    let mut processes = Vec::with_capacity(relevant_pids.len());
    let mut same_process_dependency_output = 0;
    let mut same_process_cache_output = 0;
    let mut same_process_network_cache = 0;
    let mut same_process_network_cache_output = 0;
    let mut ancestor_dependency_output = 0;
    let mut ancestor_cache_output = 0;
    let mut ancestor_network_output = 0;
    let mut ancestor_network_cache_output = 0;
    let mut dependency_mmaps = 0;
    let mut cache_mmaps = 0;
    let mut output_publications = 0;
    let mut temp_output_publications = 0;
    let mut lineage_temp_output_publications = 0;

    for raw_pid in &relevant_pids {
        let item = activity.get(raw_pid).cloned().unwrap_or_default();
        let has_dependency = item.has_dependency();
        let has_cache = item.has_cache();
        let has_output = item.has_output();
        let has_network = item.has_network();
        if has_dependency && has_output {
            same_process_dependency_output += 1;
        }
        if has_cache && has_output {
            same_process_cache_output += 1;
        }
        if has_network && has_cache {
            same_process_network_cache += 1;
        }
        if has_network && has_cache && has_output {
            same_process_network_cache_output += 1;
        }

        if has_output {
            let mut current = raw_pid;
            let mut seen = BTreeSet::new();
            let mut ancestor_dependency = false;
            let mut ancestor_cache = false;
            let mut ancestor_network = false;
            while let Some(parent) = parents.get(current) {
                if !seen.insert(current.clone()) {
                    break;
                }
                if let Some(parent_activity) = activity.get(parent) {
                    ancestor_dependency |= parent_activity.has_dependency();
                    ancestor_cache |= parent_activity.has_cache();
                    ancestor_network |= parent_activity.has_network();
                }
                current = parent;
            }
            if ancestor_dependency {
                ancestor_dependency_output += 1;
            }
            if ancestor_cache {
                ancestor_cache_output += 1;
            }
            if ancestor_network {
                ancestor_network_output += 1;
            }
            if ancestor_network && ancestor_cache {
                ancestor_network_cache_output += 1;
            }
        }

        dependency_mmaps += item.dependency_mmaps;
        cache_mmaps += item.cache_mmaps;
        output_publications += item.output_publications;
        temp_output_publications += item.temp_output_publications;
        lineage_temp_output_publications += item.lineage_temp_output_publications;

        let parent_process = parents
            .get(raw_pid)
            .and_then(|parent| anonymous.get(parent))
            .cloned();
        let role = roles
            .get(raw_pid)
            .cloned()
            .filter(|value| !value.is_empty())
            .or_else(|| (!item.role.is_empty()).then_some(item.role.clone()))
            .unwrap_or_else(|| "unknown".to_string());
        processes.push(ProcessActivitySummary {
            process: anonymous
                .get(raw_pid)
                .cloned()
                .expect("relevant process must have anonymous identifier"),
            parent_process,
            lineage_depth: lineage_depth(raw_pid, &parents),
            role,
            dependency_reads: item.dependency_reads,
            dependency_mmaps: item.dependency_mmaps,
            cache_reads: item.cache_reads,
            cache_mmaps: item.cache_mmaps,
            output_writes: item.output_writes,
            output_publications: item.output_publications,
            temp_output_publications: item.temp_output_publications,
            lineage_temp_output_publications: item.lineage_temp_output_publications,
            successful_nonlocal_network_events: item.successful_nonlocal_network_events,
        });
    }

    ProcessTraceSummary {
        attempted: true,
        tracer_available: true,
        truncated,
        parsed_bytes,
        processes,
        same_process_dependency_output,
        same_process_cache_output,
        same_process_network_cache,
        same_process_network_cache_output,
        ancestor_dependency_output,
        ancestor_cache_output,
        ancestor_network_output,
        ancestor_network_cache_output,
        dependency_mmaps,
        cache_mmaps,
        output_publications,
        temp_output_publications,
        lineage_temp_output_publications,
        error: None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct CachePhaseSummary {
    file_count: usize,
    total_bytes: u64,
    aggregate_sha256: Option<String>,
    truncated: bool,
    max_files: usize,
    max_bytes: u64,
}

fn parse_dependency_cache_phase(path: &Path) -> BTreeMap<String, CachePhaseSummary> {
    let Ok(raw) = fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    let mut values = BTreeMap::new();
    for line in raw.lines() {
        let mut parts = line.splitn(7, '\t');
        let Some(ecosystem) = parts.next().filter(|value| !value.is_empty()) else {
            continue;
        };
        let Some(file_count) = parts.next().and_then(|value| value.parse::<usize>().ok()) else {
            continue;
        };
        let Some(total_bytes) = parts.next().and_then(|value| value.parse::<u64>().ok()) else {
            continue;
        };
        let aggregate_sha256 = parts
            .next()
            .filter(|value| value.len() == 64)
            .map(str::to_string);
        let Some(truncated) = parts.next().and_then(|value| match value {
            "0" => Some(false),
            "1" => Some(true),
            _ => None,
        }) else {
            continue;
        };
        let Some(max_files) = parts.next().and_then(|value| value.parse::<usize>().ok()) else {
            continue;
        };
        let Some(max_bytes) = parts.next().and_then(|value| value.parse::<u64>().ok()) else {
            continue;
        };
        values.insert(
            ecosystem.to_string(),
            CachePhaseSummary {
                file_count,
                total_bytes,
                aggregate_sha256,
                truncated,
                max_files,
                max_bytes,
            },
        );
    }
    values
}

fn parse_dependency_cache_summaries(
    metadata_dir: &Path,
) -> (Vec<DependencyCacheSummary>, Option<String>) {
    let status = fs::read_to_string(metadata_dir.join("dependency-status")).unwrap_or_default();
    if status.trim() == "unavailable" {
        return (
            Vec::new(),
            Some("dependency cache summarizer tools were unavailable in the build image".to_string()),
        );
    }
    if status.trim() != "available" {
        return (Vec::new(), None);
    }

    let before = parse_dependency_cache_phase(&metadata_dir.join("dependency-cache-before.tsv"));
    let after = parse_dependency_cache_phase(&metadata_dir.join("dependency-cache-after.tsv"));
    let ecosystems = before
        .keys()
        .chain(after.keys())
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let mut summaries = Vec::new();
    for ecosystem in ecosystems {
        let before_value = before.get(&ecosystem).cloned().unwrap_or_default();
        let after_value = after.get(&ecosystem).cloned().unwrap_or_default();
        let max_files = before_value.max_files.max(after_value.max_files);
        let max_bytes = before_value.max_bytes.max(after_value.max_bytes);
        let comparison_complete = !before_value.truncated && !after_value.truncated;
        let observed_change = before_value.file_count != after_value.file_count
            || before_value.total_bytes != after_value.total_bytes
            || before_value.aggregate_sha256 != after_value.aggregate_sha256
            || before_value.truncated != after_value.truncated;
        summaries.push(DependencyCacheSummary {
            ecosystem,
            before_file_count: before_value.file_count,
            before_total_bytes: before_value.total_bytes,
            before_aggregate_sha256: before_value.aggregate_sha256,
            before_truncated: before_value.truncated,
            after_file_count: after_value.file_count,
            after_total_bytes: after_value.total_bytes,
            after_aggregate_sha256: after_value.aggregate_sha256,
            after_truncated: after_value.truncated,
            max_files,
            max_bytes,
            changed_during_build: comparison_complete && observed_change,
            observed_change,
            comparison_complete,
        });
    }
    (summaries, None)
}

fn dependency_network_correlation(
    network: &NetworkTraceSummary,
    caches: &[DependencyCacheSummary],
    attempted: bool,
) -> DependencyNetworkCorrelation {
    if !attempted {
        return DependencyNetworkCorrelation::default();
    }
    let successful_nonlocal_network_events = network
        .successful_endpoint_scopes
        .iter()
        .filter(|(scope, _)| is_nonlocal_scope(scope.as_str()))
        .map(|(_, count)| *count)
        .sum::<usize>();
    let complete_cache_mutations = caches
        .iter()
        .filter(|cache| cache.changed_during_build)
        .count();
    let incomplete_cache_observations = caches
        .iter()
        .filter(|cache| !cache.comparison_complete)
        .count();
    DependencyNetworkCorrelation {
        attempted: true,
        network_trace_available: network.tracer_available,
        successful_nonlocal_network_events,
        complete_cache_mutations,
        incomplete_cache_observations,
        build_window_cooccurrence: network.tracer_available
            && successful_nonlocal_network_events > 0
            && complete_cache_mutations > 0,
    }
}

fn collect_runtime_dependency_provenance(
    workspace: &Path,
    metadata_dir: &Path,
    attempted: bool,
) -> RuntimeDependencyProvenance {
    if !attempted {
        return RuntimeDependencyProvenance::default();
    }
    let (dependency_files, dependency_resolutions, mut errors) =
        match collect_dependency_provenance(workspace) {
            Ok((files, resolutions)) => (files, resolutions, Vec::new()),
            Err(error) => (Vec::new(), Vec::new(), vec![format!("post-build dependency provenance failed: {error:#}")]),
        };
    let (cache_summaries, cache_error) = parse_dependency_cache_summaries(metadata_dir);
    if let Some(error) = cache_error {
        errors.push(error);
    }
    RuntimeDependencyProvenance {
        attempted: true,
        dependency_files,
        dependency_resolutions,
        cache_summaries,
        network_cache_correlation: DependencyNetworkCorrelation::default(),
        error: (!errors.is_empty()).then(|| errors.join("; ")),
    }
}

fn apply_source_mtime(root: &Path, epoch: i64) -> Result<()> {
    let timestamp = FileTime::from_unix_time(epoch, 0);
    for entry in WalkDir::new(root).follow_links(false).contents_first(true) {
        let entry = entry
            .with_context(|| format!("cannot walk {} while setting source mtimes", root.display()))?;
        if entry.file_type().is_symlink() {
            continue;
        }
        set_file_mtime(entry.path(), timestamp)
            .with_context(|| format!("cannot set source mtime on {}", entry.path().display()))?;
    }
    Ok(())
}

impl Runner for OciRunner {
    fn available(&self) -> Result<()> {
        let runtime = self.runtime_executable();
        let output = Command::new(runtime)
            .arg("version")
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .with_context(|| {
                format!(
                    "{} is required but the `{runtime}` executable was not found",
                    self.backend.display_name()
                )
            })?;

        if !output.status.success() {
            bail!(
                "{} is installed but unavailable: {}",
                self.backend.display_name(),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }

    fn run_attempt(
        &self,
        spec: &BuildSpec,
        experiment_id: Uuid,
        source_digest: &str,
        ordinal: usize,
        environment: &ControlledEnvironment,
    ) -> Result<RunOutcome> {
        let requested_image = environment
            .image_override
            .as_deref()
            .unwrap_or(&spec.image);
        let resolved_image_id = self.ensure_image(requested_image)?;
        let mut toolchain_provenance = self.toolchain_provenance(&resolved_image_id);
        let workspace = self.prepare_workspace(environment)?;
        let metadata_dir = tempfile::Builder::new()
            .prefix("reprobisect-meta-")
            .tempdir()
            .context("cannot create temporary runner metadata directory")?;
        let runtime_toolchain_bindings =
            self.prepare_toolchain_wrappers(metadata_dir.path(), environment)?;
        self.prepare_dependency_cache_spec(metadata_dir.path(), environment)?;
        let source_overrides = self.apply_source_overrides(workspace.path(), environment)?;
        if let Some(epoch) = environment.source_mtime_epoch {
            apply_source_mtime(workspace.path(), epoch)?;
        }
        let run_id = Uuid::new_v4();
        let container_name = format!("reprobisect-{}", run_id.simple());
        let mut runtime_environment = spec.environment.clone();
        runtime_environment.extend(environment.environment.clone());
        for (key, value) in &runtime_toolchain_bindings {
            runtime_environment.insert(key.clone(), value.clone());
        }
        if let Some(cpu_count) = environment.cpu_count {
            runtime_environment.insert(
                "REPROBISECT_CPU_COUNT".to_string(),
                cpu_count.to_string(),
            );
        }

        let mut recorded_environment: BTreeMap<String, String> = spec
            .environment
            .keys()
            .map(|key| (key.clone(), "<configured>".to_string()))
            .collect();
        recorded_environment.extend(environment.environment.clone());
        for (key, value) in &environment.toolchain_bindings {
            recorded_environment.insert(key.clone(), expand_controlled_value(value, environment));
        }
        if let Some(cpu_count) = environment.cpu_count {
            recorded_environment.insert(
                "REPROBISECT_CPU_COUNT".to_string(),
                cpu_count.to_string(),
            );
        }

        let expanded_command = expand_command(&spec.command, environment);
        let args = self.runtime_args(
            spec,
            workspace.path(),
            metadata_dir.path(),
            environment,
            &runtime_environment,
            &resolved_image_id,
            &container_name,
            &expanded_command,
        )?;

        let started = Instant::now();
        let mut child = Command::new(self.runtime_executable())
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| {
                format!(
                    "failed to start {} build run {ordinal}",
                    self.backend.display_name()
                )
            })?;
        let stdout_pipe = child
            .stdout
            .take()
            .context("OCI runtime stdout pipe was not available")?;
        let stderr_pipe = child
            .stderr
            .take()
            .context("OCI runtime stderr pipe was not available")?;
        let log_limit = spec.log_capture_max_bytes;
        let stdout_reader = thread::spawn(move || drain_bounded_stream(stdout_pipe, log_limit));
        let stderr_reader = thread::spawn(move || drain_bounded_stream(stderr_pipe, log_limit));

        let finished = Arc::new(AtomicBool::new(false));
        let watchdog_finished = Arc::clone(&finished);
        let watchdog_name = container_name.clone();
        let watchdog_runtime = self.runtime_executable().to_string();
        let timeout = Duration::from_secs(spec.timeout_seconds);
        let watchdog = thread::spawn(move || {
            let start = Instant::now();
            while start.elapsed() < timeout {
                if watchdog_finished.load(Ordering::Relaxed) {
                    return false;
                }
                thread::sleep(Duration::from_millis(100));
            }

            if !watchdog_finished.swap(true, Ordering::Relaxed) {
                let _ = Command::new(&watchdog_runtime)
                    .arg("kill")
                    .arg(&watchdog_name)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
                true
            } else {
                false
            }
        });

        let status_result = child.wait();
        finished.store(true, Ordering::Relaxed);
        let timed_out = watchdog
            .join()
            .map_err(|_| anyhow::anyhow!("build timeout watchdog panicked"))?;
        let stdout_capture = stdout_reader
            .join()
            .map_err(|_| anyhow::anyhow!("stdout capture thread panicked"))?
            .context("failed while draining build stdout")?;
        let stderr_capture = stderr_reader
            .join()
            .map_err(|_| anyhow::anyhow!("stderr capture thread panicked"))?
            .context("failed while draining build stderr")?;
        let status = status_result.with_context(|| {
            format!(
                "failed waiting for {} build run {ordinal}",
                self.backend.display_name()
            )
        })?;

        if timed_out {
            bail!(
                "build run {ordinal} exceeded timeout of {} seconds",
                spec.timeout_seconds
            );
        }

        let exit_code = status.code().unwrap_or(128);
        let stdout = stdout_capture.text;
        let stderr = stderr_capture.text;
        toolchain_provenance.bindings = parse_toolchain_binding_probes(
            &metadata_dir.path().join("toolchain-bindings.tsv"),
            &metadata_dir.path().join("toolchain-invocations"),
        );
        let network_trace = parse_network_trace(
            metadata_dir.path(),
            environment.network_trace,
            environment.syscall_trace_max_bytes,
        );
        let process_trace = parse_process_trace(
            metadata_dir.path(),
            environment.file_input_trace,
            environment.network_trace,
            environment.syscall_trace_max_bytes,
            &spec.outputs,
            environment,
        );
        let mut runtime_dependency_provenance = collect_runtime_dependency_provenance(
            workspace.path(),
            metadata_dir.path(),
            environment.runtime_dependency_provenance,
        );
        runtime_dependency_provenance.network_cache_correlation = dependency_network_correlation(
            &network_trace,
            &runtime_dependency_provenance.cache_summaries,
            environment.runtime_dependency_provenance && environment.network_trace,
        );

        if !status.success() {
            return Ok(RunOutcome::BuildFailed(BuildFailure {
                schema_version: BUILD_EVIDENCE_SCHEMA_CURRENT,
                experiment_id,
                run_id,
                ordinal,
                runner_backend: self.backend,
                source_digest: source_digest.to_string(),
                image: requested_image.to_string(),
                resolved_image_id,
                command: expanded_command,
                working_directory: spec.working_directory.clone(),
                effective_environment: recorded_environment,
                controlled_environment: environment.clone(),
                toolchain_provenance,
                source_overrides,
                network_trace,
                process_trace,
                runtime_dependency_provenance,
                exit_code,
                duration_ms: started.elapsed().as_millis(),
                log_capture_max_bytes: spec.log_capture_max_bytes,
                stdout,
                stderr,
                stdout_sha256: stdout_capture.sha256,
                stderr_sha256: stderr_capture.sha256,
                stdout_bytes: stdout_capture.total_bytes,
                stderr_bytes: stderr_capture.total_bytes,
                stdout_truncated: stdout_capture.truncated,
                stderr_truncated: stderr_capture.truncated,
            }));
        }

        let artifacts = collect_artifacts(workspace.path(), &spec.outputs, environment)?;

        Ok(RunOutcome::Success(BuildRun {
            schema_version: BUILD_EVIDENCE_SCHEMA_CURRENT,
            experiment_id,
            run_id,
            ordinal,
            runner_backend: self.backend,
            source_digest: source_digest.to_string(),
            image: requested_image.to_string(),
            resolved_image_id,
            command: expanded_command,
            working_directory: spec.working_directory.clone(),
            effective_environment: recorded_environment,
            controlled_environment: environment.clone(),
            toolchain_provenance,
            source_overrides,
            network_trace,
            process_trace,
            runtime_dependency_provenance,
            exit_code,
            duration_ms: started.elapsed().as_millis(),
            log_capture_max_bytes: spec.log_capture_max_bytes,
            stdout,
            stderr,
            stdout_sha256: stdout_capture.sha256,
            stderr_sha256: stderr_capture.sha256,
            stdout_bytes: stdout_capture.total_bytes,
            stderr_bytes: stderr_capture.total_bytes,
            stdout_truncated: stdout_capture.truncated,
            stderr_truncated: stderr_capture.truncated,
            artifacts,
        }))
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_log_capture_drains_and_hashes_full_stream() {
        let data = b"abcdefghij";
        let capture = drain_bounded_stream(std::io::Cursor::new(data), 4).unwrap();
        assert_eq!(capture.text, "abcd");
        assert_eq!(capture.total_bytes, 10);
        assert!(capture.truncated);
        let mut hasher = Sha256::new();
        hasher.update(data);
        assert_eq!(capture.sha256, hex::encode(hasher.finalize()));
    }

    #[test]
    fn bounded_log_capture_handles_empty_stream() {
        let capture = drain_bounded_stream(std::io::Cursor::new(Vec::<u8>::new()), 4).unwrap();
        assert_eq!(capture.text, "");
        assert_eq!(capture.total_bytes, 0);
        assert!(!capture.truncated);
        let mut hasher = Sha256::new();
        hasher.update([]);
        assert_eq!(capture.sha256, hex::encode(hasher.finalize()));
    }

    #[test]
    fn expands_source_and_build_placeholders() {
        let environment = ControlledEnvironment {
            container_source_path: "/src-alt".into(),
            container_work_path: "/build-alt".into(),
            ..ControlledEnvironment::default()
        };
        let expanded = expand_command(
            &["cc".into(), "{source}/main.c".into(), "-o".into(), "{build}/app".into()],
            &environment,
        );
        assert_eq!(expanded[1], "/src-alt/main.c");
        assert_eq!(expanded[3], "/build-alt/app");
        assert_eq!(
            expand_controlled_value("{source}/cc", &environment),
            "/src-alt/cc"
        );
    }

    #[test]
    fn selects_configured_oci_runtime() {
        let project = tempfile::tempdir().unwrap();
        let docker = OciRunner::new(project.path().to_path_buf(), RunnerBackend::Docker);
        let podman = OciRunner::new(project.path().to_path_buf(), RunnerBackend::Podman);
        assert_eq!(docker.runtime_executable(), "docker");
        assert_eq!(podman.runtime_executable(), "podman");
    }

    #[test]
    fn dependency_override_preserves_target_metadata() {
        let project = tempfile::tempdir().unwrap();
        fs::write(project.path().join("requirements.txt"), "demo==1\n").unwrap();
        fs::write(project.path().join("requirements.variant.txt"), "demo==2\n").unwrap();
        let fixed = FileTime::from_unix_time(946_684_800, 0);
        set_file_times(project.path().join("requirements.txt"), fixed, fixed).unwrap();

        let runner = OciRunner::new(project.path().to_path_buf(), RunnerBackend::Docker);
        let workspace = runner.prepare_workspace(&ControlledEnvironment::default()).unwrap();
        let before = fs::metadata(workspace.path().join("requirements.txt")).unwrap();
        let mut environment = ControlledEnvironment::default();
        environment.source_file_overrides.insert(
            PathBuf::from("requirements.txt"),
            PathBuf::from("requirements.variant.txt"),
        );
        let records = runner.apply_source_overrides(workspace.path(), &environment).unwrap();
        let after = fs::metadata(workspace.path().join("requirements.txt")).unwrap();

        assert_eq!(fs::read_to_string(workspace.path().join("requirements.txt")).unwrap(), "demo==2\n");
        assert_ne!(records[0].baseline_sha256, records[0].variant_sha256);
        assert_eq!(before.permissions().readonly(), after.permissions().readonly());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(before.permissions().mode(), after.permissions().mode());
        }
        assert_eq!(FileTime::from_last_modification_time(&before), FileTime::from_last_modification_time(&after));
    }

    #[test]
    fn runtime_args_grant_ptrace_only_when_tracing_is_requested() {
        let project = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let metadata = tempfile::tempdir().unwrap();
        let runner = OciRunner::new(project.path().to_path_buf(), RunnerBackend::Docker);
        let spec = BuildSpec {
            image: "debian:bookworm".into(),
            command: vec!["true".into()],
            outputs: vec![PathBuf::from("out")],
            environment: BTreeMap::new(),
            working_directory: PathBuf::from("."),
            timeout_seconds: 60,
            log_capture_max_bytes: 1024 * 1024,
        };
        let effective = BTreeMap::new();
        let command = vec!["true".to_string()];

        let baseline = ControlledEnvironment::default();
        let args = runner
            .runtime_args(
                &spec,
                workspace.path(),
                metadata.path(),
                &baseline,
                &effective,
                "sha256:demo",
                "reprobisect-test",
                &command,
            )
            .unwrap();
        assert!(!args.windows(2).any(|pair| pair[0] == "--cap-add" && pair[1] == "SYS_PTRACE"));

        let traced = ControlledEnvironment {
            file_input_trace: true,
            ..ControlledEnvironment::default()
        };
        let args = runner
            .runtime_args(
                &spec,
                workspace.path(),
                metadata.path(),
                &traced,
                &effective,
                "sha256:demo",
                "reprobisect-test",
                &command,
            )
            .unwrap();
        assert!(args.windows(2).any(|pair| pair[0] == "--cap-add" && pair[1] == "SYS_PTRACE"));
    }

    #[test]
    fn parses_redacted_network_trace_summary() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("trace-status"), "available\n").unwrap();
        fs::write(
            temp.path().join("syscall.trace"),
            "1 socket(AF_INET, SOCK_STREAM, IPPROTO_TCP) = 3\n1 connect(3, {sa_family=AF_INET, sin_port=htons(443), sin_addr=inet_addr(\"203.0.113.5\")}, 16) = 0\n2 sendto(4, \"x\", 1, 0, {sa_family=AF_INET6, sin6_addr=inet_pton(AF_INET6, \"::1\")}, 28) = 1\n2 recvfrom(4, \"x\", 1, 0, NULL, NULL) = 1\n",
        )
        .unwrap();
        let summary = parse_network_trace(temp.path(), true, 1024 * 1024);
        assert!(summary.tracer_available);
        assert_eq!(summary.socket_calls, 1);
        assert_eq!(summary.connect_calls, 1);
        assert_eq!(summary.successful_connects, 1);
        assert_eq!(summary.failed_connects, 0);
        assert_eq!(summary.sendto_calls, 1);
        assert_eq!(summary.recvfrom_calls, 1);
        assert_eq!(summary.address_families, vec!["AF_INET".to_string(), "AF_INET6".to_string()]);
        assert_eq!(summary.endpoint_scopes.get("public"), Some(&1));
        assert_eq!(summary.endpoint_scopes.get("loopback"), Some(&1));
        assert_eq!(summary.successful_endpoint_scopes.get("public"), Some(&1));
        assert_eq!(summary.successful_endpoint_scopes.get("loopback"), Some(&1));
    }

    #[test]
    fn process_trace_correlates_reads_network_and_output_without_persisting_paths() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("trace-status"), "available\n").unwrap();
        fs::write(
            temp.path().join("syscall.trace"),
            concat!(
                "100 execve(\"/usr/bin/cargo\", [\"cargo\"], 0x0) = 0\n",
                "100 openat(AT_FDCWD, \"Cargo.lock\", O_RDONLY|O_CLOEXEC) = 3\n",
                "100 openat(AT_FDCWD, \"/root/.cargo/registry/cache/pkg\", O_RDONLY) = 4\n",
                "100 connect(5, {sa_family=AF_INET, sin_addr=inet_addr(\"203.0.113.9\")}, 16) = 0\n",
                "100 openat(AT_FDCWD, \"/workspace/out.bin\", O_WRONLY|O_CREAT|O_TRUNC, 0666) = 6\n",
            ),
        )
        .unwrap();
        let environment = ControlledEnvironment {
            file_input_trace: true,
            network_trace: true,
            ..ControlledEnvironment::default()
        };
        let summary = parse_process_trace(
            temp.path(),
            true,
            true,
            1024 * 1024,
            &[PathBuf::from("out.bin")],
            &environment,
        );
        assert!(summary.tracer_available);
        assert_eq!(summary.processes.len(), 1);
        assert_eq!(summary.processes[0].process, "p1");
        assert_eq!(summary.processes[0].role, "package_manager");
        assert_eq!(summary.processes[0].dependency_reads, 1);
        assert_eq!(summary.processes[0].cache_reads, 1);
        assert_eq!(summary.processes[0].output_writes, 1);
        assert_eq!(summary.processes[0].successful_nonlocal_network_events, 1);
        assert_eq!(summary.same_process_dependency_output, 1);
        assert_eq!(summary.same_process_cache_output, 1);
        assert_eq!(summary.same_process_network_cache_output, 1);
        let serialized = serde_json::to_string(&summary).unwrap();
        assert!(!serialized.contains("Cargo.lock"));
        assert!(!serialized.contains("203.0.113.9"));
        assert!(!serialized.contains("/root/.cargo"));
    }

    #[test]
    fn process_trace_tracks_dependency_mmap_and_tempfile_publication() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("trace-status"), "available\n").unwrap();
        fs::write(
            temp.path().join("syscall.trace"),
            concat!(
                "100 execve(\"/usr/bin/python3\", [\"python3\"], 0x0) = 0\n",
                "100 openat(AT_FDCWD, \"requirements.txt\", O_RDONLY|O_CLOEXEC) = 3\n",
                "100 mmap(NULL, 4096, PROT_READ, MAP_PRIVATE, 3, 0) = 0x7f1234500000\n",
                "100 openat(AT_FDCWD, \"build/.out.tmp\", O_WRONLY|O_CREAT|O_TRUNC, 0666) = 4\n",
                "100 rename(\"build/.out.tmp\", \"/workspace/build/out.bin\") = 0\n",
            ),
        )
        .unwrap();
        let summary = parse_process_trace(
            temp.path(),
            true,
            false,
            1024 * 1024,
            &[PathBuf::from("build/out.bin")],
            &ControlledEnvironment::default(),
        );
        assert!(summary.tracer_available);
        assert_eq!(summary.processes.len(), 1);
        assert_eq!(summary.processes[0].dependency_reads, 1);
        assert_eq!(summary.processes[0].dependency_mmaps, 1);
        assert_eq!(summary.processes[0].output_writes, 0);
        assert_eq!(summary.processes[0].output_publications, 1);
        assert_eq!(summary.processes[0].temp_output_publications, 1);
        assert_eq!(summary.dependency_mmaps, 1);
        assert_eq!(summary.output_publications, 1);
        assert_eq!(summary.temp_output_publications, 1);
        assert_eq!(summary.same_process_dependency_output, 1);
        let serialized = serde_json::to_string(&summary).unwrap();
        assert!(!serialized.contains("requirements.txt"));
        assert!(!serialized.contains(".out.tmp"));
        assert!(!serialized.contains("/workspace/build/out.bin"));
    }

    #[test]
    fn process_trace_anonymizes_parent_child_lineage_and_inherited_descriptors() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("trace-status"), "available\n").unwrap();
        fs::write(
            temp.path().join("syscall.trace"),
            concat!(
                "100 execve(\"/bin/sh\", [\"sh\"], 0x0) = 0\n",
                "100 openat(AT_FDCWD, \"requirements.txt\", O_RDONLY|O_CLOEXEC) = 3\n",
                "100 clone(child_stack=NULL, flags=SIGCHLD) = 101\n",
                "101 fcntl(3, F_SETFD, 0) = 0\n",
                "101 execve(\"/usr/bin/python3\", [\"python3\"], 0x0) = 0\n",
                "101 mmap(NULL, 8192, PROT_READ, MAP_PRIVATE, 3, 0) = 0x7f1234600000\n",
                "101 openat(AT_FDCWD, \"/workspace/out.bin\", O_WRONLY|O_CREAT|O_TRUNC, 0666) = 4\n",
            ),
        )
        .unwrap();
        let summary = parse_process_trace(
            temp.path(),
            true,
            false,
            1024 * 1024,
            &[PathBuf::from("out.bin")],
            &ControlledEnvironment::default(),
        );
        assert_eq!(summary.processes.len(), 2);
        let parent = summary
            .processes
            .iter()
            .find(|process| process.parent_process.is_none())
            .unwrap();
        let child = summary
            .processes
            .iter()
            .find(|process| process.parent_process.is_some())
            .unwrap();
        assert_eq!(child.parent_process.as_deref(), Some(parent.process.as_str()));
        assert_eq!(child.lineage_depth, 1);
        assert_eq!(child.role, "interpreter");
        assert_eq!(child.dependency_mmaps, 1);
        assert_eq!(child.output_writes, 1);
        assert_eq!(summary.ancestor_dependency_output, 1);
        assert_eq!(summary.same_process_dependency_output, 1);
        assert!(summary.processes.iter().all(|process| process.process.starts_with('p')));
    }

    #[test]
    fn process_trace_drops_cloexec_descriptor_on_exec_without_clear() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("trace-status"), "available\n").unwrap();
        fs::write(
            temp.path().join("syscall.trace"),
            concat!(
                "300 openat(AT_FDCWD, \"requirements.txt\", O_RDONLY|O_CLOEXEC) = 3\n",
                "300 clone(child_stack=NULL, flags=SIGCHLD) = 301\n",
                "301 execve(\"/usr/bin/python3\", [\"python3\"], 0x0) = 0\n",
                "301 mmap(NULL, 4096, PROT_READ, MAP_PRIVATE, 3, 0) = 0x7f1234700000\n",
                "301 openat(AT_FDCWD, \"/workspace/out.bin\", O_WRONLY|O_CREAT|O_TRUNC, 0666) = 4\n",
            ),
        )
        .unwrap();
        let summary = parse_process_trace(
            temp.path(),
            true,
            false,
            1024 * 1024,
            &[PathBuf::from("out.bin")],
            &ControlledEnvironment::default(),
        );
        let child = summary
            .processes
            .iter()
            .find(|process| process.parent_process.is_some())
            .unwrap();
        assert_eq!(child.dependency_mmaps, 0);
        assert_eq!(summary.ancestor_dependency_output, 1);
        // The ancestor open remains useful lineage evidence, but it must not be
        // converted into a false child mmap observation after CLOEXEC.
        assert_eq!(summary.dependency_mmaps, 0);
    }

    #[test]
    fn process_trace_fcntl_clear_preserves_inherited_descriptor_across_exec() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("trace-status"), "available\n").unwrap();
        fs::write(
            temp.path().join("syscall.trace"),
            concat!(
                "400 openat(AT_FDCWD, \"requirements.txt\", O_RDONLY|O_CLOEXEC) = 3\n",
                "400 clone(child_stack=NULL, flags=SIGCHLD) = 401\n",
                "401 fcntl(3, F_SETFD, 0) = 0\n",
                "401 execve(\"/usr/bin/python3\", [\"python3\"], 0x0) = 0\n",
                "401 mmap(NULL, 4096, PROT_READ, MAP_PRIVATE, 3, 0) = 0x7f1234800000\n",
                "401 openat(AT_FDCWD, \"/workspace/out.bin\", O_WRONLY|O_CREAT|O_TRUNC, 0666) = 4\n",
            ),
        )
        .unwrap();
        let summary = parse_process_trace(
            temp.path(),
            true,
            false,
            1024 * 1024,
            &[PathBuf::from("out.bin")],
            &ControlledEnvironment::default(),
        );
        let child = summary
            .processes
            .iter()
            .find(|process| process.parent_process.is_some())
            .unwrap();
        assert_eq!(child.dependency_mmaps, 1);
        assert_eq!(summary.dependency_mmaps, 1);
    }

    #[test]
    fn process_trace_correlates_cross_process_tempfile_publication_only_with_lineage() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("trace-status"), "available\n").unwrap();
        fs::write(
            temp.path().join("syscall.trace"),
            concat!(
                "200 clone(child_stack=NULL, flags=SIGCHLD) = 201\n",
                "201 openat(AT_FDCWD, \"build/.handoff\", O_WRONLY|O_CREAT|O_TRUNC, 0666) = 3\n",
                "200 rename(\"build/.handoff\", \"/workspace/out.bin\") = 0\n",
            ),
        )
        .unwrap();
        let summary = parse_process_trace(
            temp.path(),
            true,
            false,
            1024 * 1024,
            &[PathBuf::from("out.bin")],
            &ControlledEnvironment::default(),
        );
        assert_eq!(summary.output_publications, 1);
        assert_eq!(summary.temp_output_publications, 0);
        assert_eq!(summary.lineage_temp_output_publications, 1);
        let serialized = serde_json::to_string(&summary).unwrap();
        assert!(!serialized.contains(".handoff"));
        assert!(!serialized.contains("/workspace/out.bin"));
    }

    #[test]
    fn failed_file_open_is_not_counted_as_a_read() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("trace-status"), "available\n").unwrap();
        fs::write(
            temp.path().join("syscall.trace"),
            "42 openat(AT_FDCWD, \"requirements.txt\", O_RDONLY) = -1 ENOENT (No such file or directory)\n",
        )
        .unwrap();
        let summary = parse_process_trace(
            temp.path(),
            true,
            false,
            1024 * 1024,
            &[PathBuf::from("out")],
            &ControlledEnvironment::default(),
        );
        assert!(summary.processes.is_empty());
        assert_eq!(summary.same_process_dependency_output, 0);
    }

    #[test]
    fn configured_cache_paths_do_not_serialize_in_controlled_environment() {
        let mut environment = ControlledEnvironment::default();
        environment
            .dependency_cache_paths
            .insert("private".to_string(), "/private/customer/cache".to_string());
        let serialized = serde_json::to_string(&environment).unwrap();
        assert!(!serialized.contains("/private/customer/cache"));
        assert!(!serialized.contains("dependency_cache_paths"));
    }

    #[test]
    fn network_only_trace_does_not_create_file_process_summary() {
        let summary = parse_process_trace(
            Path::new("/does/not/matter"),
            false,
            true,
            1024 * 1024,
            &[],
            &ControlledEnvironment::default(),
        );
        assert!(!summary.attempted);
        assert!(summary.processes.is_empty());
    }

    #[test]
    fn parses_dependency_cache_before_after_summaries() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("dependency-status"), "available\n").unwrap();
        fs::write(
            temp.path().join("dependency-cache-before.tsv"),
            "cargo\t2\t20\taaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
        )
        .unwrap();
        fs::write(
            temp.path().join("dependency-cache-after.tsv"),
            concat!(
                "cargo\t3\t30\tbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n",
                "demo\t1\t7\tf2d4517b0e2664ef422e3bda2c0ed892dbec3f63895aebcf740619971ec61642\n",
            ),
        )
        .unwrap();

        let (summaries, error) = parse_dependency_cache_summaries(temp.path());
        assert!(error.is_none());
        let cargo = summaries.iter().find(|item| item.ecosystem == "cargo").unwrap();
        assert_eq!(cargo.before_file_count, 2);
        assert_eq!(cargo.after_file_count, 3);
        assert!(cargo.changed_during_build);
        assert!(cargo.comparison_complete);
        assert!(!cargo.before_truncated);
        assert!(!cargo.after_truncated);
        let demo = summaries.iter().find(|item| item.ecosystem == "demo").unwrap();
        assert_eq!(demo.before_file_count, 0);
        assert_eq!(demo.after_file_count, 1);
        assert!(demo.before_aggregate_sha256.is_none());
        assert!(demo.changed_during_build);
        assert!(demo.comparison_complete);
    }

    #[test]
    fn truncated_cache_samples_are_not_promoted_to_confirmed_mutations() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("dependency-status"), "available\n").unwrap();
        fs::write(
            temp.path().join("dependency-cache-before.tsv"),
            "cargo\t2\t20\taaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\t1\t2\t20\n",
        )
        .unwrap();
        fs::write(
            temp.path().join("dependency-cache-after.tsv"),
            "cargo\t2\t20\tbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\t1\t2\t20\n",
        )
        .unwrap();
        let (summaries, error) = parse_dependency_cache_summaries(temp.path());
        assert!(error.is_none());
        let cargo = &summaries[0];
        assert!(cargo.observed_change);
        assert!(!cargo.comparison_complete);
        assert!(!cargo.changed_during_build);
    }

    #[test]
    fn correlates_only_successful_nonlocal_network_with_complete_cache_mutation() {
        let mut scopes = BTreeMap::new();
        scopes.insert("public".to_string(), 2);
        scopes.insert("loopback".to_string(), 3);
        let network = NetworkTraceSummary {
            attempted: true,
            tracer_available: true,
            successful_endpoint_scopes: scopes,
            ..NetworkTraceSummary::default()
        };
        let cache = DependencyCacheSummary {
            ecosystem: "cargo".into(),
            before_file_count: 1,
            before_total_bytes: 4,
            before_aggregate_sha256: Some("a".repeat(64)),
            before_truncated: false,
            after_file_count: 2,
            after_total_bytes: 8,
            after_aggregate_sha256: Some("b".repeat(64)),
            after_truncated: false,
            max_files: 10,
            max_bytes: 1024,
            changed_during_build: true,
            observed_change: true,
            comparison_complete: true,
        };
        let correlation = dependency_network_correlation(&network, &[cache], true);
        assert_eq!(correlation.successful_nonlocal_network_events, 2);
        assert_eq!(correlation.complete_cache_mutations, 1);
        assert!(correlation.build_window_cooccurrence);
    }

    #[test]
    fn parses_toolchain_binding_probe_rows() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        fs::write(
            temp.path(),
            "CC\t/src/toolchains/cc-a\t/src/toolchains/cc-a\tgcc (GCC) 14\n",
        )
        .unwrap();
        let invocations = tempfile::tempdir().unwrap();
        fs::write(invocations.path().join("CC.log"), ".\n.\n").unwrap();
        let probes = parse_toolchain_binding_probes(temp.path(), invocations.path());
        assert_eq!(probes.len(), 1);
        assert_eq!(probes[0].variable, "CC");
        assert_eq!(probes[0].version.as_deref(), Some("gcc (GCC) 14"));
        assert_eq!(probes[0].invocation_count, 2);
    }
}
