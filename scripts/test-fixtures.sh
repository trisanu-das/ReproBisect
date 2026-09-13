#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${ROOT}/target/debug/reprobisect"

cargo build --locked --manifest-path "${ROOT}/Cargo.toml"

cleanup_fixture() {
  rm -rf "${ROOT}/fixtures/$1/.reprobisect"
}

check_fixture() {
  local name="$1"
  local expected_status="$2"
  local expected_variable="${3:-}"
  local output
  output="$(mktemp)"

  set +e
  "${BIN}" check "${ROOT}/fixtures/${name}" --format json >"${output}"
  local exit_code=$?
  set -e

  python3 - "${output}" "${name}" "${expected_status}" "${expected_variable}" "${exit_code}" <<'PY'
import json
import sys

path, name, expected_status, expected_variable, exit_code = sys.argv[1:]
with open(path, "r", encoding="utf-8") as handle:
    report = json.load(handle)

status = report["status"]
if status != expected_status:
    raise SystemExit(f"{name}: expected status {expected_status!r}, got {status!r}")

expected_exit = 0 if expected_status == "reproducible" else 1
if int(exit_code) != expected_exit:
    raise SystemExit(f"{name}: expected exit {expected_exit}, got {exit_code}")

if expected_variable:
    diagnosed = {
        variable
        for diagnosis in report.get("diagnoses", [])
        for variable in diagnosis.get("causal_variables", [])
    }
    if expected_variable not in diagnosed:
        raise SystemExit(
            f"{name}: expected diagnosis for {expected_variable!r}; got {sorted(diagnosed)!r}"
        )

if name == "pe-timestamp":
    semantic = [
        difference
        for result in report.get("interventions", [])
        for delta in result.get("artifact_deltas", [])
        for difference in delta.get("semantic_evidence", [])
        if difference.get("field") == "coff_timestamp"
    ]
    if not semantic:
        raise SystemExit("pe-timestamp: missing semantic COFF timestamp evidence")
    if not any(
        "deterministic timestamp" in remediation.lower()
        for diagnosis in report.get("diagnoses", [])
        for remediation in diagnosis.get("remediation", [])
    ):
        raise SystemExit("pe-timestamp: missing PE/COFF deterministic-timestamp remediation")

print(f"PASS {name}: {status}" + (f" cause={expected_variable}" if expected_variable else ""))
PY

  rm -f "${output}"
  cleanup_fixture "${name}"
}

check_parallel_statistics() {
  local name="parallel-order"
  local output
  output="$(mktemp)"

  set +e
  "${BIN}" check "${ROOT}/fixtures/${name}" --format json >"${output}"
  local exit_code=$?
  set -e

  python3 - "${output}" "${exit_code}" <<'PY_STATS'
import json
import sys

path, exit_code = sys.argv[1:]
report = json.load(open(path, encoding="utf-8"))
if report["status"] != "non_reproducible":
    raise SystemExit(f"parallel-order: status={report['status']!r}")
if int(exit_code) != 1:
    raise SystemExit(f"parallel-order: exit={exit_code}")

matches = [
    item for item in report.get("interventions", [])
    if item.get("intervention", {}).get("variable") == "cpu_count"
]
if len(matches) != 1:
    raise SystemExit(f"parallel-order: expected one cpu_count intervention, got {len(matches)}")
result = matches[0]
evidence = result.get("stochastic_effect") or {}
if evidence.get("classification") != "supported":
    raise SystemExit(f"parallel-order: stochastic evidence not supported: {evidence!r}")
if evidence.get("baseline_changed_trials") != 0:
    raise SystemExit(f"parallel-order: matched baseline became unstable: {evidence!r}")
if evidence.get("fisher_exact_p_value", 1.0) > evidence.get("alpha", 0.0):
    raise SystemExit(f"parallel-order: p-value exceeds alpha: {evidence!r}")
if len(result.get("reference_runs", [])) != evidence.get("baseline_trials"):
    raise SystemExit("parallel-order: matched baseline runs were not persisted")

diagnosed = {
    variable
    for diagnosis in report.get("diagnoses", [])
    for variable in diagnosis.get("causal_variables", [])
}
if "cpu_count" not in diagnosed:
    raise SystemExit(f"parallel-order: expected cpu_count diagnosis, got {sorted(diagnosed)!r}")
print(
    "PASS parallel-order: matched stochastic effect "
    f"p={evidence['fisher_exact_p_value']:.6g}"
)
PY_STATS

  rm -f "${output}"
  cleanup_fixture "${name}"
}

check_interaction_fixture() {
  local name="interaction-two-vars"
  local output
  output="$(mktemp)"

  set +e
  "${BIN}" check "${ROOT}/fixtures/${name}" --format json >"${output}"
  local exit_code=$?
  set -e

  python3 - "${output}" "${exit_code}" <<'PY'
import json
import sys

path, exit_code = sys.argv[1:]
report = json.load(open(path, encoding="utf-8"))
if report["status"] != "non_reproducible":
    raise SystemExit(f"interaction-two-vars: status={report['status']!r}")
if int(exit_code) != 1:
    raise SystemExit(f"interaction-two-vars: exit={exit_code}")

matches = [
    diagnosis
    for diagnosis in report.get("diagnoses", [])
    if set(diagnosis.get("causal_variables", [])) == {"A", "B"}
]
if not matches:
    raise SystemExit(
        "interaction-two-vars: expected minimized interaction diagnosis for A+B; "
        f"got {[d.get('causal_variables') for d in report.get('diagnoses', [])]!r}"
    )
interaction = report.get("interaction_search") or {}
if set(interaction.get("minimal_variables", [])) != {"A", "B"}:
    raise SystemExit(f"interaction-two-vars: unexpected ddmin result {interaction!r}")
print("PASS interaction-two-vars: cause=A+B")
PY

  rm -f "${output}"
  cleanup_fixture "${name}"
}

check_fix_verification() {
  local name="$1"
  local expected_strategy="$2"
  local expect_project_patch="${3:-false}"
  local output
  local makefile_hash_before=""
  local makefile_hash_after=""
  output="$(mktemp)"

  if [[ "${expect_project_patch}" == "true" ]]; then
    makefile_hash_before="$(sha256sum "${ROOT}/fixtures/${name}/Makefile" | awk '{print $1}')"
  fi

  set +e
  "${BIN}" fix "${ROOT}/fixtures/${name}" --verify --format json >"${output}"
  local exit_code=$?
  set -e

  if [[ "${expect_project_patch}" == "true" ]]; then
    makefile_hash_after="$(sha256sum "${ROOT}/fixtures/${name}/Makefile" | awk '{print $1}')"
    if [[ "${makefile_hash_before}" != "${makefile_hash_after}" ]]; then
      echo "${name}: fix verification modified the original Makefile" >&2
      exit 1
    fi
  fi

  python3 - "${output}" "${name}" "${expected_strategy}" "${exit_code}" "${expect_project_patch}" <<'PYFIX'
import json
import sys

path, name, expected_strategy, exit_code, expect_project_patch = sys.argv[1:]
report = json.load(open(path, encoding="utf-8"))
if int(exit_code) != 0:
    raise SystemExit(f"{name}: expected successful verified fix exit, got {exit_code}")
if report["diagnosis"]["status"] != "non_reproducible":
    raise SystemExit(f"{name}: diagnosis did not reproduce the original failure")
verified = [item for item in report.get("verifications", []) if item.get("verified")]
if not verified:
    raise SystemExit(f"{name}: no verified fix in {report.get('verifications')!r}")
matching = [
    candidate
    for candidate in report.get("candidates", [])
    if candidate.get("strategy") == expected_strategy
]
matching_ids = {candidate.get("id") for candidate in matching}
if not matching_ids:
    raise SystemExit(
        f"{name}: expected strategy {expected_strategy!r}; "
        f"got {[c.get('strategy') for c in report.get('candidates', [])]!r}"
    )
if expect_project_patch == "true" and not any(candidate.get("project_patch") for candidate in matching):
    raise SystemExit(f"{name}: expected a project-level patch candidate for {expected_strategy}")
if not any(item.get("plan_id") in matching_ids and item.get("verified") for item in report.get("verifications", [])):
    raise SystemExit(f"{name}: expected verified {expected_strategy} plan")
print(f"PASS {name}: {expected_strategy} verified")
PYFIX

  rm -f "${output}"
  cleanup_fixture "${name}"
}


check_toolchain_invocation_fixture() {
  local name="$1"
  local expected_variable="$2"
  local binding="$3"
  local output
  output="$(mktemp)"

  set +e
  "${BIN}" check "${ROOT}/fixtures/${name}" --format json >"${output}"
  local exit_code=$?
  set -e

  python3 - "${output}" "${name}" "${expected_variable}" "${binding}" "${exit_code}" <<'PYTOOL'
import json
import sys

path, name, expected_variable, binding, exit_code = sys.argv[1:]
report = json.load(open(path, encoding="utf-8"))
if report["status"] != "non_reproducible" or int(exit_code) != 1:
    raise SystemExit(f"{name}: unexpected status/exit: {report['status']!r}/{exit_code}")
results = [
    item for item in report.get("interventions", [])
    if item.get("intervention", {}).get("variable") == expected_variable
]
if len(results) != 1:
    raise SystemExit(f"{name}: expected one {expected_variable} intervention, got {len(results)}")
result = results[0]
variant_counts = []
for run in result.get("runs", []):
    probes = run.get("toolchain_provenance", {}).get("bindings", [])
    matches = [probe for probe in probes if probe.get("variable") == binding]
    if not matches:
        raise SystemExit(f"{name}: missing {binding} binding probe in variant run")
    variant_counts.append(matches[0].get("invocation_count", 0))
confirmation = result.get("confirmation_run") or {}
base = [
    probe
    for probe in confirmation.get("toolchain_provenance", {}).get("bindings", [])
    if probe.get("variable") == binding
]
if not variant_counts or any(count <= 0 for count in variant_counts) or not base or base[0].get("invocation_count", 0) <= 0:
    raise SystemExit(
        f"{name}: tool invocation not proven: variant={variant_counts!r} baseline={base!r}"
    )
diagnoses = [
    d for d in report.get("diagnoses", [])
    if expected_variable in d.get("causal_variables", [])
]
if not diagnoses or diagnoses[0].get("confidence") != "high":
    raise SystemExit(f"{name}: expected high-confidence invoked-tool diagnosis: {diagnoses!r}")
print(f"PASS {name}: {binding} invoked and causally changed output")
PYTOOL

  rm -f "${output}"
  cleanup_fixture "${name}"
}

check_runtime_dependency_provenance() {
  local name="runtime-dependency-provenance"
  local output
  output="$(mktemp)"
  "${BIN}" check "${ROOT}/fixtures/${name}" --format json >"${output}"

  python3 - "${output}" <<'PYDEP'
import json
import sys

report = json.load(open(sys.argv[1], encoding="utf-8"))
if report["status"] != "reproducible":
    raise SystemExit(f"runtime-dependency-provenance: status={report['status']!r}")
for run in report.get("runs", []):
    provenance = run.get("runtime_dependency_provenance", {})
    if not provenance.get("attempted"):
        raise SystemExit("runtime-dependency-provenance: provenance was not attempted")
    resolutions = provenance.get("dependency_resolutions", [])
    requirements = [item for item in resolutions if item.get("path") == "requirements.txt"]
    if not requirements or requirements[0].get("package_count") != 2:
        raise SystemExit(f"runtime-dependency-provenance: unexpected resolution summary {resolutions!r}")
    caches = [item for item in provenance.get("cache_summaries", []) if item.get("ecosystem") == "demo"]
    if not caches:
        raise SystemExit(f"runtime-dependency-provenance: demo cache summary missing {caches!r}")
    cache = caches[0]
    if cache.get("before_file_count") != 0 or cache.get("after_file_count") != 1:
        raise SystemExit(f"runtime-dependency-provenance: unexpected before/after cache counts {cache!r}")
    if cache.get("before_aggregate_sha256") is not None:
        raise SystemExit(f"runtime-dependency-provenance: cache unexpectedly existed before build {cache!r}")
    if len(cache.get("after_aggregate_sha256") or "") != 64:
        raise SystemExit("runtime-dependency-provenance: post-build aggregate cache digest missing")
    if not cache.get("changed_during_build"):
        raise SystemExit(f"runtime-dependency-provenance: cache change was not detected {cache!r}")
print("PASS runtime-dependency-provenance: post-build resolution + before/after cache content fingerprinted")
PYDEP

  rm -f "${output}"
  cleanup_fixture "${name}"
}

check_bounded_runtime_dependency_provenance() {
  local name="runtime-dependency-bounded"
  local output
  output="$(mktemp)"
  "${BIN}" check "${ROOT}/fixtures/${name}" --format json >"${output}"

  python3 - "${output}" <<'PYBOUND'
import json
import sys

report = json.load(open(sys.argv[1], encoding="utf-8"))
if report["status"] != "reproducible":
    raise SystemExit(f"runtime-dependency-bounded: status={report['status']!r}")
for run in report.get("runs", []):
    caches = [
        item
        for item in run.get("runtime_dependency_provenance", {}).get("cache_summaries", [])
        if item.get("ecosystem") == "bounded"
    ]
    if not caches:
        raise SystemExit("runtime-dependency-bounded: bounded cache summary missing")
    cache = caches[0]
    if cache.get("max_files") != 1 or cache.get("max_bytes") != 1048576:
        raise SystemExit(f"runtime-dependency-bounded: wrong limits {cache!r}")
    if not cache.get("after_truncated"):
        raise SystemExit(f"runtime-dependency-bounded: expected truncated post-build sample {cache!r}")
    if cache.get("comparison_complete"):
        raise SystemExit(f"runtime-dependency-bounded: truncated sample marked complete {cache!r}")
    if cache.get("changed_during_build"):
        raise SystemExit(f"runtime-dependency-bounded: incomplete sample promoted to confirmed mutation {cache!r}")
    if not cache.get("observed_change"):
        raise SystemExit(f"runtime-dependency-bounded: bounded observation did not record possible change {cache!r}")
print("PASS runtime-dependency-bounded: truncation prevents false confirmed mutation")
PYBOUND

  rm -f "${output}"
  cleanup_fixture "${name}"
}

check_multi_provenance_fixture() {
  local name="provenance-multi"
  local output
  output="$(mktemp)"
  "${BIN}" check "${ROOT}/fixtures/${name}" --format json >"${output}"

  python3 - "${output}" <<'PYMULTI'
import json
import sys

report = json.load(open(sys.argv[1], encoding="utf-8"))
if report["status"] != "reproducible":
    raise SystemExit(f"provenance-multi: status={report['status']!r}")
records = report.get("source_provenance", {}).get("dependency_resolutions", [])
parsed = {item.get("ecosystem") for item in records if item.get("parsed")}
expected = {"poetry", "uv", "pipenv", "go", "composer", "gradle", "bundler", "yarn", "pnpm"}
missing = expected - parsed
if missing:
    raise SystemExit(f"provenance-multi: missing parsed ecosystems {sorted(missing)!r}; got {sorted(parsed)!r}")
for item in records:
    if item.get("parsed") and len(item.get("normalized_sha256") or "") != 64:
        raise SystemExit(f"provenance-multi: invalid normalized hash {item!r}")
print("PASS provenance-multi: broader lockfile provenance remains normalized/redacted")
PYMULTI

  rm -f "${output}"
  cleanup_fixture "${name}"
}

check_cargo_fix_verification() {
  local name="fixable-path-cargo"
  local output
  output="$(mktemp)"
  rm -rf "${ROOT}/fixtures/${name}/.cargo"

  set +e
  "${BIN}" fix "${ROOT}/fixtures/${name}" --verify --format json >"${output}"
  local exit_code=$?
  set -e

  if [[ -e "${ROOT}/fixtures/${name}/.cargo/config.toml" ]]; then
    echo "${name}: verification created .cargo/config.toml in the original project" >&2
    exit 1
  fi

  python3 - "${output}" "${exit_code}" <<'PYCARGO'
import json
import sys

report = json.load(open(sys.argv[1], encoding="utf-8"))
if int(sys.argv[2]) != 0:
    raise SystemExit(f"fixable-path-cargo: fix command exit={sys.argv[2]}")
matching = [
    candidate for candidate in report.get("candidates", [])
    if candidate.get("strategy") == "compiler-prefix-map"
]
if not matching:
    raise SystemExit("fixable-path-cargo: compiler-prefix-map candidate missing")
patch = matching[0].get("project_patch") or {}
if patch.get("path") != ".cargo/config.toml":
    raise SystemExit(f"fixable-path-cargo: unexpected patch path {patch!r}")
operation = patch.get("operation") or {}
if operation.get("kind") != "create":
    raise SystemExit(f"fixable-path-cargo: expected create patch, got {operation!r}")
matching_ids = {candidate.get("id") for candidate in matching}
if not any(
    item.get("plan_id") in matching_ids and item.get("verified")
    for item in report.get("verifications", [])
):
    raise SystemExit(f"fixable-path-cargo: project patch did not verify: {report.get('verifications')!r}")
print("PASS fixable-path-cargo: temporary Cargo config path-remap patch verified")
PYCARGO

  rm -f "${output}"
  cleanup_fixture "${name}"
}

check_provenance_fixture() {
  local name="provenance-lock"
  local output
  output="$(mktemp)"
  "${BIN}" check "${ROOT}/fixtures/${name}" --format json >"${output}"

  python3 - "${output}" <<'PY'
import json
import sys

report = json.load(open(sys.argv[1], encoding="utf-8"))
if report["status"] != "reproducible":
    raise SystemExit(f"provenance-lock: status={report['status']!r}")
records = report.get("source_provenance", {}).get("dependency_files", [])
paths = {record.get("path") for record in records}
if "requirements.txt" not in paths:
    raise SystemExit(f"provenance-lock: requirements.txt fingerprint missing: {records!r}")
if not records or any(len(record.get("sha256", "")) != 64 for record in records):
    raise SystemExit("provenance-lock: invalid dependency fingerprint")
resolutions = report.get("source_provenance", {}).get("dependency_resolutions", [])
requirements = [item for item in resolutions if item.get("path") == "requirements.txt"]
if not requirements or not requirements[0].get("parsed"):
    raise SystemExit(f"provenance-lock: parsed requirements provenance missing: {resolutions!r}")
if len(requirements[0].get("normalized_sha256") or "") != 64:
    raise SystemExit("provenance-lock: normalized dependency-resolution hash missing")
runs = report.get("runs", [])
if not runs or not runs[0].get("resolved_image_id"):
    raise SystemExit("provenance-lock: resolved image ID missing")
probes = runs[0].get("toolchain_provenance", {}).get("probes", [])
if not any(probe.get("tool") in {"gcc", "cc"} for probe in probes):
    raise SystemExit(f"provenance-lock: compiler toolchain probe missing: {probes!r}")
print("PASS provenance-lock: source + image + toolchain provenance")
PY

  rm -f "${output}"
  cleanup_fixture "${name}"
}

check_file_input_provenance() {
  local name="file-input-provenance"
  local output
  output="$(mktemp)"
  "${BIN}" check "${ROOT}/fixtures/${name}" --format json >"${output}"

  python3 - "${output}" <<'PYTRACE'
import json
import sys

path = sys.argv[1]
raw = open(path, encoding="utf-8").read()
report = json.loads(raw)
if report["status"] != "reproducible":
    raise SystemExit(f"file-input-provenance: status={report['status']!r}")
private_root = "/tmp/reprobisect-private-cache"
if private_root in raw or "dependency_cache_paths" in raw:
    raise SystemExit("file-input-provenance: private cache-root configuration leaked into persisted JSON")
for run in report.get("runs", []):
    trace = run.get("process_trace", {})
    if not trace.get("attempted"):
        raise SystemExit("file-input-provenance: process trace was not attempted")
    if not trace.get("tracer_available"):
        raise SystemExit(f"file-input-provenance: tracer unavailable: {trace.get('error')!r}")
    processes = trace.get("processes", [])
    if not any(
        p.get("dependency_reads", 0) > 0
        and p.get("cache_reads", 0) > 0
        and p.get("output_writes", 0) > 0
        for p in processes
    ):
        raise SystemExit(f"file-input-provenance: same anonymous process did not observe dependency+cache+output activity: {processes!r}")
    if trace.get("same_process_dependency_output", 0) < 1:
        raise SystemExit(f"file-input-provenance: dependency/output co-occurrence missing: {trace!r}")
    if trace.get("same_process_cache_output", 0) < 1:
        raise SystemExit(f"file-input-provenance: cache/output co-occurrence missing: {trace!r}")
print("PASS file-input-provenance: same-process reads/output observed without persisting private paths")
PYTRACE

  rm -f "${output}"
  cleanup_fixture "${name}"
}

check_file_input_lineage() {
  local name="file-input-lineage"
  local output
  output="$(mktemp)"
  "${BIN}" check "${ROOT}/fixtures/${name}" --format json >"${output}"

  python3 - "${output}" <<'PYLINEAGE'
import json
import sys

path = sys.argv[1]
raw = open(path, encoding="utf-8").read()
report = json.loads(raw)
if report["status"] != "reproducible":
    raise SystemExit(f"file-input-lineage: status={report['status']!r}")
for forbidden in [
    "/tmp/reprobisect-lineage-private-cache",
    ".handoff.tmp",
    "dependency_cache_paths",
]:
    if forbidden in raw:
        raise SystemExit(f"file-input-lineage: private/raw trace detail leaked: {forbidden!r}")

for run in report.get("runs", []):
    trace = run.get("process_trace", {})
    if not trace.get("tracer_available"):
        raise SystemExit(f"file-input-lineage: tracer unavailable: {trace.get('error')!r}")
    if trace.get("dependency_mmaps", 0) < 1:
        raise SystemExit(f"file-input-lineage: dependency-backed mmap missing: {trace!r}")
    if trace.get("cache_mmaps", 0) < 1:
        raise SystemExit(f"file-input-lineage: cache-backed mmap missing: {trace!r}")
    if trace.get("ancestor_dependency_output", 0) < 1:
        raise SystemExit(f"file-input-lineage: ancestor dependency/output lineage missing: {trace!r}")
    if trace.get("ancestor_cache_output", 0) < 1:
        raise SystemExit(f"file-input-lineage: ancestor cache/output lineage missing: {trace!r}")
    if trace.get("output_publications", 0) < 1:
        raise SystemExit(f"file-input-lineage: rename publication missing: {trace!r}")
    if trace.get("lineage_temp_output_publications", 0) < 1:
        raise SystemExit(f"file-input-lineage: cross-process tempfile publication missing: {trace!r}")
    processes = trace.get("processes", [])
    children = [p for p in processes if p.get("parent_process")]
    if not children:
        raise SystemExit(f"file-input-lineage: anonymous parent/child edge missing: {processes!r}")
    ids = {p.get("process") for p in processes}
    if any(not isinstance(value, str) or not value.startswith("p") for value in ids):
        raise SystemExit(f"file-input-lineage: non-anonymous process identity persisted: {ids!r}")
    if any(child.get("parent_process") not in ids for child in children):
        raise SystemExit(f"file-input-lineage: dangling anonymous parent reference: {processes!r}")
print("PASS file-input-lineage: mmap, lineage, and tempfile publication observed without raw identities/paths")
PYLINEAGE

  rm -f "${output}"
  cleanup_fixture "${name}"
}

check_artifact_formats() {
  local name="artifact-formats"
  local output
  output="$(mktemp)"
  "${BIN}" check "${ROOT}/fixtures/${name}" --format json >"${output}"

  python3 - "${output}" <<'PYFORMATS'
import json
import sys

report = json.load(open(sys.argv[1], encoding="utf-8"))
if report["status"] != "reproducible":
    raise SystemExit(f"artifact-formats: status={report['status']!r}")
runs = report.get("runs", [])
if not runs:
    raise SystemExit("artifact-formats: missing control runs")
expected = {
    "build/demo.jar": ("jar", "jar", "jar", {
        "manifest_present": "true", "class_count": "1", "manifest_version": "1.0",
    }),
    "build/demo-1.0-py3-none-any.whl": ("python_wheel", "python_wheel", "python_wheel", {
        "dist_info_count": "1", "wheel_version": "1.0", "metadata_version": "2.1",
        "record_present": "true",
    }),
    "build/demo.deb": ("deb", "deb", "deb", {
        "debian_binary_version": "2.0", "control_archive": "control.tar", "data_archive": "data.tar",
    }),
    "build/demo-oci.tar": ("oci_image", "oci_image_tar", "oci_image", {
        "image_layout_version": "1.0.0", "index_schema_version": "2", "manifest_count": "0", "blob_count": "1",
    }),
    "build/demo.exe": ("pe_coff", None, "pe_coff", {"coff_timestamp": "0", "section_count": "0"}),
    "build/demo.macho": ("mach_o", None, "mach_o", {
        "endianness": "little", "is_64_bit": "true", "load_command_count": "0",
    }),
    "build/demo.wasm": ("wasm", None, "wasm", {"version": "1", "section_count": "0"}),
}
for run in runs:
    artifacts = {item["logical_path"]: item for item in run.get("artifacts", [])}
    if set(artifacts) != set(expected):
        raise SystemExit(f"artifact-formats: unexpected outputs {sorted(artifacts)!r}")
    for path, (artifact_type, archive_format, semantic_kind, semantic_attributes) in expected.items():
        item = artifacts[path]
        if item.get("detected_type") != artifact_type:
            raise SystemExit(
                f"artifact-formats: {path} type={item.get('detected_type')!r}, expected={artifact_type!r}"
            )
        metadata = item.get("archive_metadata")
        if archive_format is None:
            if metadata is not None:
                raise SystemExit(f"artifact-formats: {path} unexpectedly has archive metadata")
        elif not metadata or metadata.get("format") != archive_format:
            raise SystemExit(
                f"artifact-formats: {path} archive format={None if not metadata else metadata.get('format')!r}"
            )
        semantic = item.get("semantic_metadata") or {}
        if semantic.get("kind") != semantic_kind:
            raise SystemExit(f"artifact-formats: {path} semantic kind={semantic.get('kind')!r}")
        attributes = semantic.get("attributes") or {}
        for key, value in semantic_attributes.items():
            if attributes.get(key) != value:
                raise SystemExit(
                    f"artifact-formats: {path} semantic {key}={attributes.get(key)!r}, expected={value!r}"
                )
        for key in ("manifest_content_sha256", "wheel_content_sha256", "metadata_content_sha256", "record_content_sha256", "index_json_sha256"):
            if key in attributes and len(attributes[key]) != 64:
                raise SystemExit(f"artifact-formats: {path} malformed semantic digest {key}")
print("PASS artifact-formats: structural classification plus bounded semantic metadata")
PYFORMATS

  rm -f "${output}"
  cleanup_fixture "${name}"
}

check_log_capture() {
  local name="log-capture"
  local output
  output="$(mktemp)"
  "${BIN}" check "${ROOT}/fixtures/${name}" --format json >"${output}"

  python3 - "${output}" <<'PYLOGCAP'
import hashlib
import json
import sys

report = json.load(open(sys.argv[1], encoding="utf-8"))
if report.get("status") != "reproducible":
    raise SystemExit(f"log-capture: status={report.get('status')!r}")
expected_stdout = hashlib.sha256(b"A" * (2 * 1024 * 1024)).hexdigest()
expected_stderr = hashlib.sha256(b"B" * (2 * 1024 * 1024)).hexdigest()
for run in report.get("runs", []):
    if run.get("log_capture_max_bytes") != 65536:
        raise SystemExit(f"log-capture: cap={run.get('log_capture_max_bytes')!r}")
    if run.get("stdout_bytes") != 2 * 1024 * 1024 or run.get("stderr_bytes") != 2 * 1024 * 1024:
        raise SystemExit(f"log-capture: total byte counts incorrect")
    if run.get("stdout_truncated") is not True or run.get("stderr_truncated") is not True:
        raise SystemExit("log-capture: truncation flags missing")
    if len(run.get("stdout", "").encode()) != 65536 or len(run.get("stderr", "").encode()) != 65536:
        raise SystemExit("log-capture: retained text is not bounded to 65536 bytes")
    if run.get("stdout_sha256") != expected_stdout or run.get("stderr_sha256") != expected_stderr:
        raise SystemExit("log-capture: full-stream SHA-256 mismatch")
print("PASS log-capture: retained text bounded; complete-stream hashes exact")
PYLOGCAP

  rm -f "${output}"
  cleanup_fixture "${name}"
}

check_environment_compare() {
  local name="environment-compare"
  local output
  output="$(mktemp)"

  set +e
  "${BIN}" compare "${ROOT}/fixtures/${name}" \
    --good good.toml --bad bad.toml --format json >"${output}"
  local exit_code=$?
  set -e

  python3 - "${output}" "${exit_code}" <<'PYCOMPARE'
import json
import sys

report = json.load(open(sys.argv[1], encoding="utf-8"))
if report.get("status") != "minimized":
    raise SystemExit(f"environment-compare: status={report.get('status')!r}")
if int(sys.argv[2]) != 1:
    raise SystemExit(f"environment-compare: expected exit 1, got {sys.argv[2]}")
minimal = {item.get("variable") for item in report.get("minimal_delta", [])}
if minimal != {"environment:A", "environment:B"}:
    raise SystemExit(f"environment-compare: unexpected minimal delta {minimal!r}")
all_delta = {item.get("variable") for item in report.get("delta", [])}
if "environment:NOISE" not in all_delta:
    raise SystemExit(f"environment-compare: irrelevant changed variable missing from full delta {all_delta!r}")
if report.get("reverted_to_good") is not True:
    raise SystemExit(f"environment-compare: good reversion failed {report.get('reverted_to_good')!r}")
bad_runs = report.get("bad_runs", [])
repro = report.get("reproduction_runs", [])
if not bad_runs or not repro:
    raise SystemExit("environment-compare: endpoint/reproduction runs missing")
bad_signature = [(a["logical_path"], a["sha256"]) for a in bad_runs[0]["artifacts"]]
for run in repro:
    signature = [(a["logical_path"], a["sha256"]) for a in run["artifacts"]]
    if signature != bad_signature:
        raise SystemExit("environment-compare: minimized subset did not reproduce exact bad signature")
print("PASS environment-compare: ddmin retained interacting A+B and discarded NOISE")
PYCOMPARE

  rm -f "${output}"
  cleanup_fixture "${name}"
}

check_environment_compare_failure() {
  local name="environment-compare-failure"
  local output
  output="$(mktemp)"

  set +e
  "${BIN}" compare "${ROOT}/fixtures/${name}" \
    --good good.toml --bad bad.toml --format json >"${output}"
  local exit_code=$?
  set -e

  python3 - "${output}" "${exit_code}" <<'PYCOMPAREFAIL'
import json
import sys

report = json.load(open(sys.argv[1], encoding="utf-8"))
if report.get("status") != "minimized":
    raise SystemExit(f"environment-compare-failure: status={report.get('status')!r}")
if int(sys.argv[2]) != 1:
    raise SystemExit(f"environment-compare-failure: expected exit 1, got {sys.argv[2]}")
if report.get("good_outcome_kind") != "artifact_success":
    raise SystemExit(f"environment-compare-failure: good kind={report.get('good_outcome_kind')!r}")
if report.get("bad_outcome_kind") != "build_failure":
    raise SystemExit(f"environment-compare-failure: bad kind={report.get('bad_outcome_kind')!r}")
minimal = {item.get("variable") for item in report.get("minimal_delta", [])}
if minimal != {"environment:A", "environment:B"}:
    raise SystemExit(f"environment-compare-failure: unexpected minimal delta {minimal!r}")
all_delta = {item.get("variable") for item in report.get("delta", [])}
if "environment:NOISE" not in all_delta:
    raise SystemExit(f"environment-compare-failure: NOISE missing from full delta {all_delta!r}")
if report.get("bad_runs"):
    raise SystemExit("environment-compare-failure: bad endpoint unexpectedly succeeded")
bad_failures = report.get("bad_failures", [])
if len(bad_failures) != 2 or any(item.get("exit_code") != 37 for item in bad_failures):
    raise SystemExit(f"environment-compare-failure: unexpected bad failures {bad_failures!r}")
sig = report.get("bad_failure_signature") or {}
if sig.get("exit_code") != 37 or len(sig.get("stdout_sha256", "")) != 64 or len(sig.get("stderr_sha256", "")) != 64:
    raise SystemExit(f"environment-compare-failure: invalid failure signature {sig!r}")
repro = report.get("reproduction_failures", [])
if len(repro) != 2 or any(item.get("exit_code") != 37 for item in repro):
    raise SystemExit(f"environment-compare-failure: minimized subset did not reproduce failure {repro!r}")
if report.get("reproduction_runs"):
    raise SystemExit("environment-compare-failure: minimized bad subset unexpectedly succeeded")
if report.get("reverted_to_good") is not True or report.get("confirmation_run") is None:
    raise SystemExit("environment-compare-failure: successful good reversion missing")
if report.get("confirmation_failure") is not None:
    raise SystemExit("environment-compare-failure: good reversion failed")
print("PASS environment-compare-failure: ddmin minimized exact stable build-failure signature to A+B")
PYCOMPAREFAIL

  rm -f "${output}"
  cleanup_fixture "${name}"
}

check_shell_archive_fix_verification() {
  local name="fixable-archive-mtime-shell"
  local output
  local before
  local after
  output="$(mktemp)"
  before="$(sha256sum "${ROOT}/fixtures/${name}/package.sh" | awk '{print $1}')"

  set +e
  "${BIN}" fix "${ROOT}/fixtures/${name}" --verify --format json >"${output}"
  local exit_code=$?
  set -e

  after="$(sha256sum "${ROOT}/fixtures/${name}/package.sh" | awk '{print $1}')"
  if [[ "${before}" != "${after}" ]]; then
    echo "${name}: verification mutated the original package.sh" >&2
    exit 1
  fi

  python3 - "${output}" "${exit_code}" <<'PYSHELLFIX'
import json
import sys

report = json.load(open(sys.argv[1], encoding="utf-8"))
if int(sys.argv[2]) != 0:
    raise SystemExit(f"fixable-archive-mtime-shell: fix command exit={sys.argv[2]}")
matching = [
    c for c in report.get("candidates", [])
    if c.get("strategy") == "normalize-source-mtime"
]
if not matching:
    raise SystemExit("fixable-archive-mtime-shell: normalize-source-mtime candidate missing")
project = matching[0].get("project_patch") or {}
if project.get("path") != "package.sh":
    raise SystemExit(f"fixable-archive-mtime-shell: unexpected patch target {project!r}")
if (project.get("operation") or {}).get("kind") != "replace_text":
    raise SystemExit(f"fixable-archive-mtime-shell: expected replace_text patch {project!r}")
plan_ids = {c.get("id") for c in matching}
if not any(v.get("plan_id") in plan_ids and v.get("verified") for v in report.get("verifications", [])):
    raise SystemExit(f"fixable-archive-mtime-shell: project patch did not verify: {report.get('verifications')!r}")
print("PASS fixable-archive-mtime-shell: temporary shell archive patch verified without mutating checkout")
PYSHELLFIX

  rm -f "${output}"
  cleanup_fixture "${name}"
}

check_fixture reproducible-c reproducible
check_fixture timestamp-c non_reproducible SOURCE_DATE_EPOCH
check_fixture pe-timestamp non_reproducible SOURCE_DATE_EPOCH
check_fixture absolute-path-c non_reproducible build_path
check_fixture source-path-c non_reproducible source_path
check_fixture environment-leak non_reproducible BUILD_FLAVOR
check_fixture hostname-embed non_reproducible hostname
check_fixture timezone-output non_reproducible TZ
check_fixture locale-output non_reproducible locale
check_fixture archive-mtime non_reproducible source_mtime
check_fixture umask-archive non_reproducible umask
check_parallel_statistics
check_fixture build-image non_reproducible build_image
check_toolchain_invocation_fixture toolchain-executable toolchain:CC CC
check_toolchain_invocation_fixture toolchain-ar toolchain:AR AR
check_fixture dependency-lock-variant non_reproducible dependency:requirements.txt
check_fixture network-required inconclusive network_access
check_runtime_dependency_provenance
check_bounded_runtime_dependency_provenance
check_file_input_provenance
check_file_input_lineage
check_artifact_formats
check_log_capture
check_environment_compare
check_environment_compare_failure
check_interaction_fixture
check_fix_verification fixable-path-c compiler-prefix-map true
check_fix_verification timestamp-c pin-source-date-epoch true
check_fix_verification archive-mtime normalize-source-mtime
check_fix_verification umask-archive pin-umask
check_fix_verification fixable-archive-mtime-make normalize-source-mtime true
check_shell_archive_fix_verification
check_fix_verification fixable-umask-make pin-umask true
check_cargo_fix_verification

check_provenance_fixture
check_multi_provenance_fixture
