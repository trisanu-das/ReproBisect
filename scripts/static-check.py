#!/usr/bin/env python3
"""Cheap repository checks that do not require a Rust toolchain."""

from __future__ import annotations

import re
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MOD = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;\s*$")


def module_candidates(source: Path, name: str) -> tuple[Path, Path]:
    if source.name in {"main.rs", "lib.rs", "mod.rs"}:
        parent = source.parent
    else:
        parent = source.parent / source.stem
    return parent / f"{name}.rs", parent / name / "mod.rs"


def check_modules() -> None:
    missing: list[str] = []
    for source in sorted((ROOT / "src").rglob("*.rs")):
        for line_number, line in enumerate(source.read_text(encoding="utf-8").splitlines(), 1):
            match = MOD.match(line)
            if not match:
                continue
            direct, directory = module_candidates(source, match.group(1))
            if not direct.is_file() and not directory.is_file():
                missing.append(
                    f"{source.relative_to(ROOT)}:{line_number}: mod {match.group(1)}; "
                    f"has neither {direct.relative_to(ROOT)} nor {directory.relative_to(ROOT)}"
                )
    if missing:
        raise SystemExit("missing Rust modules:\n" + "\n".join(missing))


def check_toml() -> None:
    for path in sorted(ROOT.rglob("*.toml")):
        with path.open("rb") as handle:
            tomllib.load(handle)


def check_conflict_markers() -> None:
    markers = ("<" * 7, "=" * 7, ">" * 7)
    offenders: list[str] = []
    for path in sorted(ROOT.rglob("*")):
        if not path.is_file() or ".git" in path.parts or path.suffix in {".zip", ".gz"}:
            continue
        try:
            text = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue
        if any(marker in text for marker in markers):
            offenders.append(str(path.relative_to(ROOT)))
    if offenders:
        raise SystemExit("merge-conflict markers found in: " + ", ".join(offenders))



def enum_variants(source: Path, enum_name: str) -> list[str]:
    text = source.read_text(encoding="utf-8")
    match = re.search(
        rf"pub\s+enum\s+{re.escape(enum_name)}\s*\{{(?P<body>.*?)\n\}}",
        text,
        re.DOTALL,
    )
    if not match:
        raise SystemExit(f"cannot find enum {enum_name} in {source.relative_to(ROOT)}")
    variants: list[str] = []
    for raw_line in match.group("body").splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#") or line.startswith("//"):
            continue
        variant = re.match(r"([A-Za-z_][A-Za-z0-9_]*)\s*(?:[,({]|$)", line)
        if variant:
            variants.append(variant.group(1))
    return variants


def check_enum_propagation() -> None:
    model = ROOT / "src" / "model.rs"
    required: list[tuple[str, tuple[Path, ...]]] = [
        (
            "CheckStatus",
            (ROOT / "src" / "report.rs", ROOT / "src" / "cli.rs"),
        ),
        (
            "InterventionKind",
            (
                ROOT / "src" / "engine" / "diagnosis.rs",
                ROOT / "src" / "engine" / "control.rs",
            ),
        ),
        (
            "EnvironmentComparisonStatus",
            (ROOT / "src" / "report.rs", ROOT / "src" / "cli.rs"),
        ),
    ]

    missing: list[str] = []
    for enum_name, consumers in required:
        variants = enum_variants(model, enum_name)
        for consumer in consumers:
            text = consumer.read_text(encoding="utf-8")
            for variant in variants:
                token = f"{enum_name}::{variant}"
                if token not in text:
                    missing.append(
                        f"{consumer.relative_to(ROOT)} does not reference {token}"
                    )
    if missing:
        raise SystemExit("enum propagation checks failed:\n" + "\n".join(missing))


def check_intervention_match_guards() -> None:
    variants = enum_variants(ROOT / "src" / "model.rs", "InterventionKind")
    checks = [
        (ROOT / "src" / "engine" / "control.rs", "fn marker_variable_matches"),
        (ROOT / "src" / "engine" / "diagnosis.rs", "fn remediation_for"),
    ]
    failures: list[str] = []
    for path, anchor in checks:
        text = path.read_text(encoding="utf-8")
        start = text.find(anchor)
        if start < 0:
            failures.append(f"{path.relative_to(ROOT)} is missing {anchor}")
            continue
        body = text[start:]
        for variant in variants:
            token = f"InterventionKind::{variant}"
            if token not in body:
                failures.append(
                    f"{path.relative_to(ROOT)} {anchor} does not reference {token}"
                )
    if failures:
        raise SystemExit("intervention match guards failed:\n" + "\n".join(failures))


def check_cli_compile_regressions() -> None:
    cli = (ROOT / "src" / "cli.rs").read_text(encoding="utf-8")
    if "options.verbose" in cli:
        raise SystemExit(
            "src/cli.rs references options.verbose, but CheckOptions intentionally has no verbose field; use the CLI args.verbose flag"
        )
    if "pub const EXIT_DIAGNOSTIC: i32 = 1;" not in cli:
        raise SystemExit("src/cli.rs must freeze diagnostic/finding exit status at 1")
    main = (ROOT / "src" / "main.rs").read_text(encoding="utf-8")
    if "const EXIT_ERROR: i32 = 5;" not in main:
        raise SystemExit("src/main.rs must freeze operational/internal error exit status at 5")


def check_schema_literal_guards() -> None:
    # These are intentionally cheap text guards for schema evolution in environments
    # where rustc may not be available. They do not replace cargo check.
    checks = [
        ("BuildRun {", "runner_backend", (ROOT / "src" / "runner" / "oci.rs", ROOT / "src" / "evidence.rs")),
        ("BuildRun {", "toolchain_provenance", (ROOT / "src" / "runner" / "oci.rs", ROOT / "src" / "evidence.rs")),
        ("BuildRun {", "source_overrides", (ROOT / "src" / "runner" / "oci.rs", ROOT / "src" / "evidence.rs")),
        ("BuildRun {", "network_trace", (ROOT / "src" / "runner" / "oci.rs", ROOT / "src" / "evidence.rs")),
        ("BuildRun {", "process_trace", (ROOT / "src" / "runner" / "oci.rs", ROOT / "src" / "evidence.rs")),
        ("BuildRun {", "runtime_dependency_provenance", (ROOT / "src" / "runner" / "oci.rs", ROOT / "src" / "evidence.rs")),
        ("BuildFailure {", "runner_backend", (ROOT / "src" / "runner" / "oci.rs", ROOT / "src" / "evidence.rs")),
        ("BuildFailure {", "source_overrides", (ROOT / "src" / "runner" / "oci.rs", ROOT / "src" / "evidence.rs")),
        ("BuildFailure {", "network_trace", (ROOT / "src" / "runner" / "oci.rs", ROOT / "src" / "evidence.rs")),
        ("BuildFailure {", "process_trace", (ROOT / "src" / "runner" / "oci.rs", ROOT / "src" / "evidence.rs")),
        ("BuildFailure {", "runtime_dependency_provenance", (ROOT / "src" / "runner" / "oci.rs", ROOT / "src" / "evidence.rs")),
        ("BuildRun {", "log_capture_max_bytes", (ROOT / "src" / "runner" / "oci.rs", ROOT / "src" / "evidence.rs", ROOT / "src" / "engine" / "compare.rs")),
        ("BuildRun {", "stdout_sha256", (ROOT / "src" / "runner" / "oci.rs", ROOT / "src" / "evidence.rs", ROOT / "src" / "engine" / "compare.rs")),
        ("BuildRun {", "stderr_sha256", (ROOT / "src" / "runner" / "oci.rs", ROOT / "src" / "evidence.rs", ROOT / "src" / "engine" / "compare.rs")),
        ("BuildRun {", "stdout_bytes", (ROOT / "src" / "runner" / "oci.rs", ROOT / "src" / "evidence.rs", ROOT / "src" / "engine" / "compare.rs")),
        ("BuildRun {", "stderr_bytes", (ROOT / "src" / "runner" / "oci.rs", ROOT / "src" / "evidence.rs", ROOT / "src" / "engine" / "compare.rs")),
        ("BuildRun {", "stdout_truncated", (ROOT / "src" / "runner" / "oci.rs", ROOT / "src" / "evidence.rs", ROOT / "src" / "engine" / "compare.rs")),
        ("BuildRun {", "stderr_truncated", (ROOT / "src" / "runner" / "oci.rs", ROOT / "src" / "evidence.rs", ROOT / "src" / "engine" / "compare.rs")),
        ("BuildFailure {", "log_capture_max_bytes", (ROOT / "src" / "runner" / "oci.rs", ROOT / "src" / "evidence.rs", ROOT / "src" / "engine" / "compare.rs")),
        ("BuildFailure {", "stdout_sha256", (ROOT / "src" / "runner" / "oci.rs", ROOT / "src" / "evidence.rs", ROOT / "src" / "engine" / "compare.rs")),
        ("BuildFailure {", "stderr_sha256", (ROOT / "src" / "runner" / "oci.rs", ROOT / "src" / "evidence.rs", ROOT / "src" / "engine" / "compare.rs")),
        ("BuildFailure {", "stdout_bytes", (ROOT / "src" / "runner" / "oci.rs", ROOT / "src" / "evidence.rs", ROOT / "src" / "engine" / "compare.rs")),
        ("BuildFailure {", "stderr_bytes", (ROOT / "src" / "runner" / "oci.rs", ROOT / "src" / "evidence.rs", ROOT / "src" / "engine" / "compare.rs")),
        ("BuildFailure {", "stdout_truncated", (ROOT / "src" / "runner" / "oci.rs", ROOT / "src" / "evidence.rs", ROOT / "src" / "engine" / "compare.rs")),
        ("BuildFailure {", "stderr_truncated", (ROOT / "src" / "runner" / "oci.rs", ROOT / "src" / "evidence.rs", ROOT / "src" / "engine" / "compare.rs")),
        ("ArtifactRecord {", "semantic_metadata", (ROOT / "src" / "artifact" / "generic.rs", ROOT / "src" / "evidence.rs", ROOT / "src" / "engine" / "compare.rs")),
        ("ArtifactDelta {", "semantic_evidence", (ROOT / "src" / "engine" / "control.rs", ROOT / "src" / "engine" / "diagnosis.rs", ROOT / "src" / "engine" / "fix.rs")),
        ("InterventionResult {", "build_failures", (ROOT / "src" / "engine" / "control.rs", ROOT / "src" / "engine" / "diagnosis.rs", ROOT / "src" / "engine" / "fix.rs")),
        ("InterventionResult {", "reference_runs", (ROOT / "src" / "engine" / "control.rs", ROOT / "src" / "engine" / "diagnosis.rs", ROOT / "src" / "engine" / "fix.rs")),
        ("InterventionResult {", "stochastic_effect", (ROOT / "src" / "engine" / "control.rs", ROOT / "src" / "engine" / "diagnosis.rs", ROOT / "src" / "engine" / "fix.rs")),
        ("CheckReport {", "source_provenance", (ROOT / "src" / "engine" / "control.rs",)),
        ("EnvironmentComparisonReport {", "good_manifest_sha256", (ROOT / "src" / "engine" / "compare.rs",)),
        ("EnvironmentComparisonReport {", "bad_manifest_sha256", (ROOT / "src" / "engine" / "compare.rs",)),
        ("EnvironmentComparisonReport {", "minimal_delta", (ROOT / "src" / "engine" / "compare.rs",)),
        ("EnvironmentComparisonReport {", "good_outcome_kind", (ROOT / "src" / "engine" / "compare.rs",)),
        ("EnvironmentComparisonReport {", "bad_outcome_kind", (ROOT / "src" / "engine" / "compare.rs",)),
        ("EnvironmentComparisonReport {", "good_failures", (ROOT / "src" / "engine" / "compare.rs",)),
        ("EnvironmentComparisonReport {", "bad_failures", (ROOT / "src" / "engine" / "compare.rs",)),
        ("EnvironmentComparisonReport {", "bad_failure_signature", (ROOT / "src" / "engine" / "compare.rs",)),
        ("EnvironmentComparisonReport {", "reproduction_failures", (ROOT / "src" / "engine" / "compare.rs",)),
        ("EnvironmentComparisonReport {", "confirmation_failure", (ROOT / "src" / "engine" / "compare.rs",)),
    ]
    failures: list[str] = []
    for constructor, required_field, files in checks:
        for path in files:
            text = path.read_text(encoding="utf-8")
            constructors = text.count(constructor)
            fields = len(re.findall(rf"\b{re.escape(required_field)}\s*[:,]", text))
            if constructors and fields < constructors:
                failures.append(
                    f"{path.relative_to(ROOT)} has {constructors} {constructor.strip()} literal(s) "
                    f"but only {fields} {required_field} occurrence(s)"
                )
    fix_text = (ROOT / "src" / "engine" / "fix.rs").read_text(encoding="utf-8")
    fix_literals = len(re.findall(r"(?m)(?:^\s*|=\s*)FixPlan\s*\{", fix_text))
    fix_fields = len(re.findall(r"\bproject_patch\s*[:,]", fix_text)) - 1  # struct field
    if fix_fields < fix_literals:
        failures.append(
            f"src/engine/fix.rs has {fix_literals} FixPlan literal(s) but only {fix_fields} project_patch field(s)"
        )

    model_text = (ROOT / "src" / "model.rs").read_text(encoding="utf-8")
    cache_field = re.search(
        r"#\[serde\((?P<attrs>[^]]*)\)\]\s*pub\s+dependency_cache_paths\s*:",
        model_text,
        re.DOTALL,
    )
    if not cache_field or "skip_serializing" not in cache_field.group("attrs"):
        failures.append(
            "src/model.rs dependency_cache_paths must remain skip_serializing so private runtime cache roots cannot leak into evidence"
        )

    artifact_mod = (ROOT / "src" / "artifact" / "mod.rs").read_text(encoding="utf-8")
    generic_text = (ROOT / "src" / "artifact" / "generic.rs").read_text(encoding="utf-8")
    control_text = (ROOT / "src" / "engine" / "control.rs").read_text(encoding="utf-8")
    if "mod semantic;" not in artifact_mod or "compare_semantic_metadata" not in artifact_mod:
        failures.append("src/artifact/mod.rs must wire the semantic analyzer and comparator")
    if "semantic::inspect" not in generic_text:
        failures.append("src/artifact/generic.rs must populate semantic artifact metadata")
    if "compare_semantic_metadata" not in control_text:
        failures.append("src/engine/control.rs must carry semantic differences into artifact deltas")

    if failures:
        raise SystemExit("schema literal guards failed:\n" + "\n".join(failures))



def check_obvious_rust_duplicates() -> None:
    """Catch simple duplicate-definition drift without pretending to parse Rust."""
    failures: list[str] = []
    struct_re = re.compile(
        r"(?:pub\s+)?struct\s+([A-Za-z_][A-Za-z0-9_]*)\s*\{(?P<body>.*?)\n\}",
        re.DOTALL,
    )
    field_re = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*:")

    for path in sorted((ROOT / "src").rglob("*.rs")):
        text = path.read_text(encoding="utf-8")
        for match in struct_re.finditer(text):
            seen: set[str] = set()
            for raw_line in match.group("body").splitlines():
                field = field_re.match(raw_line)
                if not field:
                    continue
                name = field.group(1)
                if name in seen:
                    failures.append(
                        f"{path.relative_to(ROOT)} struct {match.group(1)} repeats field {name}"
                    )
                seen.add(name)

        previous_nonempty: tuple[int, str] | None = None
        for line_number, raw_line in enumerate(text.splitlines(), 1):
            line = raw_line.strip()
            if not line or line.startswith("//"):
                continue
            if previous_nonempty and line == previous_nonempty[1]:
                if line.startswith("let ") or ("=>" in line and line.endswith(",")):
                    failures.append(
                        f"{path.relative_to(ROOT)}:{line_number}: duplicate consecutive Rust statement: {line}"
                    )
            previous_nonempty = (line_number, line)

    if failures:
        raise SystemExit("obvious Rust duplicate checks failed:\n" + "\n".join(failures))


def check_evidence_schema_matrix() -> None:
    import json

    evidence_dir = ROOT / "tests" / "evidence"
    for path in sorted(evidence_dir.glob("*.json")):
        try:
            value = json.loads(path.read_text(encoding="utf-8"))
        except json.JSONDecodeError as error:
            raise SystemExit(f"invalid evidence JSON fixture {path.relative_to(ROOT)}: {error}") from error
        if not isinstance(value, dict) or not isinstance(value.get("schema_version"), int):
            raise SystemExit(
                f"evidence fixture {path.relative_to(ROOT)} must be an object with integer schema_version"
            )

    matrix_path = ROOT / "tests" / "evidence-schema-matrix.json"
    matrix = json.loads(matrix_path.read_text(encoding="utf-8"))
    if [row["phase"] for row in matrix] != list(range(6, 22)):
        raise SystemExit("evidence schema matrix must cover every frozen phase from 6 through 21")

    for key in ("check_report", "build_evidence", "fix_report"):
        values = [row[key] for row in matrix]
        if values != sorted(values):
            raise SystemExit(f"evidence schema matrix {key} versions must be nondecreasing")

    comparison = [row["environment_comparison"] for row in matrix]
    if any(value is not None for value in comparison[:8]):
        raise SystemExit("environment comparison evidence must begin at Phase 14")
    comparison_values = [value for value in comparison if value is not None]
    if comparison_values != sorted(comparison_values):
        raise SystemExit("environment comparison schema versions must be nondecreasing")

    schema_text = (ROOT / "src" / "schema.rs").read_text(encoding="utf-8")
    constants = {}
    for name, value in re.findall(r"pub const ([A-Z_]+): u32 = (\d+);", schema_text):
        constants[name] = int(value)

    expected = {
        "CHECK_REPORT_SCHEMA_MIN": matrix[0]["check_report"],
        "CHECK_REPORT_SCHEMA_CURRENT": matrix[-1]["check_report"],
        "BUILD_EVIDENCE_SCHEMA_MIN": matrix[0]["build_evidence"],
        "BUILD_EVIDENCE_SCHEMA_CURRENT": matrix[-1]["build_evidence"],
        "FIX_REPORT_SCHEMA_MIN": matrix[0]["fix_report"],
        "FIX_REPORT_SCHEMA_CURRENT": matrix[-1]["fix_report"],
        "ENVIRONMENT_COMPARISON_SCHEMA_MIN": comparison_values[0],
        "ENVIRONMENT_COMPARISON_SCHEMA_CURRENT": comparison_values[-1],
    }
    for name, value in expected.items():
        if constants.get(name) != value:
            raise SystemExit(
                f"src/schema.rs {name}={constants.get(name)!r}, expected {value} from historical schema matrix"
            )

    cargo = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    version = re.search(r'^version\s*=\s*"([^"]+)"', cargo, re.MULTILINE)
    if not version or version.group(1) != "1.0.0":
        raise SystemExit("Phase 21 stable source must use crate version 1.0.0")

def check_real_world_corpus() -> None:
    subprocess.run(
        [sys.executable, str(ROOT / "scripts" / "real-world-corpus.py"), "validate"],
        check=True,
    )


def check_python_syntax() -> None:
    import ast

    for path in sorted(ROOT.rglob("*.py")):
        if ".git" in path.parts:
            continue
        try:
            ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
        except SyntaxError as error:
            raise SystemExit(f"invalid Python syntax in {path.relative_to(ROOT)}: {error}") from error

def main() -> None:
    check_modules()
    check_toml()
    check_conflict_markers()
    check_enum_propagation()
    check_intervention_match_guards()
    check_cli_compile_regressions()
    check_schema_literal_guards()
    check_obvious_rust_duplicates()
    check_evidence_schema_matrix()
    check_real_world_corpus()
    check_python_syntax()
    subprocess.run(["bash", "-n", str(ROOT / "scripts" / "test-fixtures.sh")], check=True)
    print("static repository checks passed")


if __name__ == "__main__":
    main()
