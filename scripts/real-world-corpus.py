#!/usr/bin/env python3
"""Prepare and execute pinned real-world ReproBisect corpus cases.

The corpus never vendors upstream source. Each case pins a full Git commit, injects
only `.reprobisect.toml`, and hides that untracked config via `.git/info/exclude`
so Git-derived build/version logic still sees the exact upstream checkout as clean.
"""

from __future__ import annotations

import argparse
import dataclasses
import hashlib
import json
import re
import shutil
import subprocess
import sys
import tempfile
import time
import tomllib
from pathlib import Path
from typing import Any, Iterable

ROOT = Path(__file__).resolve().parents[1]
CORPUS_ROOT = ROOT / "corpus" / "real-world"
IMAGES_ROOT = ROOT / "corpus" / "images"
ID_RE = re.compile(r"^[a-z0-9][a-z0-9-]*$")
SHA_RE = re.compile(r"^[0-9a-f]{40}$")
REPO_RE = re.compile(r"^https://github\.com/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+\.git$")
TIERS = {"smoke", "extended"}
STATUSES = {"reproducible", "non_reproducible", "inconclusive", "uncontrolled_nondeterminism"}
RUNNERS = {"docker", "podman"}
OCI_DIGEST_RE = re.compile(r"^[A-Za-z0-9._/-]+@sha256:[0-9a-f]{64}$")
LOCAL_HELPER_IMAGE_RE = re.compile(r"^reprobisect-corpus/[a-z0-9._/-]+:phase21$")
CAUSAL_DIMENSIONS = {
    "build_path", "source_path", "source_date_epoch", "source_mtime",
    "timezone", "locale", "hostname", "cpu_count", "umask", "directory_order",
}


@dataclasses.dataclass(frozen=True)
class CorpusCase:
    directory: Path
    schema_version: int
    id: str
    tier: str
    description: str
    repository: str
    commit: str
    upstream_ref: str
    config_name: str
    expected_status: str
    expected_causal_variables: tuple[str, ...]
    max_seconds: int
    references: tuple[str, ...]
    helper_dockerfile: str | None = None
    helper_image: str | None = None

    @property
    def config_path(self) -> Path:
        return self.directory / self.config_name

    @property
    def helper_path(self) -> Path | None:
        if self.helper_dockerfile is None:
            return None
        return IMAGES_ROOT / self.helper_dockerfile


def run_checked(args: list[str], *, cwd: Path | None = None, timeout: int | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        args,
        cwd=cwd,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=timeout,
        check=True,
    )


def load_case(directory: Path) -> CorpusCase:
    manifest_path = directory / "case.toml"
    with manifest_path.open("rb") as handle:
        raw = tomllib.load(handle)
    allowed = {
        "schema_version", "id", "tier", "description", "repository", "commit",
        "upstream_ref", "config", "expected_status", "expected_causal_variables",
        "max_seconds", "references", "helper_dockerfile", "helper_image",
    }
    unknown = sorted(set(raw) - allowed)
    if unknown:
        raise ValueError(f"{manifest_path.relative_to(ROOT)}: unknown fields: {', '.join(unknown)}")
    return CorpusCase(
        directory=directory,
        schema_version=int(raw["schema_version"]),
        id=str(raw["id"]),
        tier=str(raw["tier"]),
        description=str(raw["description"]),
        repository=str(raw["repository"]),
        commit=str(raw["commit"]),
        upstream_ref=str(raw.get("upstream_ref", "")),
        config_name=str(raw["config"]),
        expected_status=str(raw["expected_status"]),
        expected_causal_variables=tuple(str(x) for x in raw.get("expected_causal_variables", [])),
        max_seconds=int(raw.get("max_seconds", 600)),
        references=tuple(str(x) for x in raw.get("references", [])),
        helper_dockerfile=(str(raw["helper_dockerfile"]) if raw.get("helper_dockerfile") else None),
        helper_image=(str(raw["helper_image"]) if raw.get("helper_image") else None),
    )


def load_cases() -> list[CorpusCase]:
    if not CORPUS_ROOT.is_dir():
        raise ValueError(f"missing corpus directory: {CORPUS_ROOT}")
    cases = [load_case(path) for path in sorted(CORPUS_ROOT.iterdir()) if (path / "case.toml").is_file()]
    if not cases:
        raise ValueError("real-world corpus is empty")
    return cases


def validate_case(case: CorpusCase) -> list[str]:
    errors: list[str] = []
    prefix = case.directory.relative_to(ROOT)
    if case.schema_version != 1:
        errors.append(f"{prefix}: unsupported case schema {case.schema_version}")
    if not ID_RE.fullmatch(case.id):
        errors.append(f"{prefix}: invalid id {case.id!r}")
    if case.id != case.directory.name:
        errors.append(f"{prefix}: id must equal directory name")
    if case.tier not in TIERS:
        errors.append(f"{prefix}: invalid tier {case.tier!r}")
    if not case.upstream_ref.strip():
        errors.append(f"{prefix}: upstream_ref is empty")
    if not case.description.strip():
        errors.append(f"{prefix}: description is empty")
    if not REPO_RE.fullmatch(case.repository):
        errors.append(f"{prefix}: repository must be an https://github.com/...git URL")
    if not SHA_RE.fullmatch(case.commit):
        errors.append(f"{prefix}: commit must be a full lowercase 40-hex SHA")
    if case.expected_status not in STATUSES:
        errors.append(f"{prefix}: unsupported expected_status {case.expected_status!r}")
    if case.expected_status == "reproducible" and case.expected_causal_variables:
        errors.append(f"{prefix}: reproducible case must not require causal variables")
    if case.expected_status == "non_reproducible" and not case.expected_causal_variables:
        errors.append(f"{prefix}: non_reproducible case must require at least one causal variable")
    if len(set(case.expected_causal_variables)) != len(case.expected_causal_variables):
        errors.append(f"{prefix}: duplicate expected causal variables")
    if not (30 <= case.max_seconds <= 3600):
        errors.append(f"{prefix}: max_seconds must be in 30..3600")
    if not case.config_path.is_file() or case.config_path.is_symlink():
        errors.append(f"{prefix}: config must be a regular non-symlink file: {case.config_name!r}")
    else:
        try:
            with case.config_path.open("rb") as handle:
                config = tomllib.load(handle)
            build = config.get("build", {})
            if "runner" not in build:
                errors.append(f"{prefix}: corpus config must set build.runner explicitly")
            elif build.get("runner") not in RUNNERS:
                errors.append(f"{prefix}: corpus configs must use docker/podman runner")
            if not build.get("outputs"):
                errors.append(f"{prefix}: build.outputs is empty")
            image = build.get("image")
            if not isinstance(image, str) or not image:
                errors.append(f"{prefix}: build.image is empty")
            elif case.helper_image:
                if image != case.helper_image:
                    errors.append(
                        f"{prefix}: helper_image {case.helper_image!r} must equal build.image {image!r}"
                    )
                elif not LOCAL_HELPER_IMAGE_RE.fullmatch(image):
                    errors.append(
                        f"{prefix}: local helper image must use the phase21 corpus namespace/tag: {image!r}"
                    )
            elif not OCI_DIGEST_RE.fullmatch(image):
                errors.append(
                    f"{prefix}: external corpus build.image must be pinned by sha256 digest, got {image!r}"
                )
            dimensions = config.get("experiments", {}).get("dimensions", {})
            for variable in case.expected_causal_variables:
                if variable not in CAUSAL_DIMENSIONS:
                    errors.append(f"{prefix}: unsupported expected causal variable {variable!r}")
                elif not dimensions.get(variable, False):
                    errors.append(f"{prefix}: expected variable {variable!r} is not enabled in config")
        except (tomllib.TOMLDecodeError, OSError, TypeError) as exc:
            errors.append(f"{prefix}: invalid config: {exc}")
    if bool(case.helper_dockerfile) != bool(case.helper_image):
        errors.append(f"{prefix}: helper_dockerfile and helper_image must be specified together")
    if case.helper_path is not None and (not case.helper_path.is_file() or case.helper_path.is_symlink()):
        errors.append(f"{prefix}: helper Dockerfile must be a regular non-symlink file: {case.helper_dockerfile!r}")
    elif case.helper_path is not None:
        try:
            dockerfile = case.helper_path.read_text(encoding="utf-8")
            from_lines = [
                line.strip() for line in dockerfile.splitlines()
                if line.strip().upper().startswith("FROM ")
            ]
            if len(from_lines) != 1:
                errors.append(f"{prefix}: helper Dockerfile must contain exactly one FROM instruction")
            else:
                base = from_lines[0].split(None, 1)[1].split()[0]
                if not OCI_DIGEST_RE.fullmatch(base):
                    errors.append(
                        f"{prefix}: helper Dockerfile base image must be pinned by sha256 digest, got {base!r}"
                    )
        except OSError as exc:
            errors.append(f"{prefix}: cannot read helper Dockerfile: {exc}")
    if not case.references:
        errors.append(f"{prefix}: at least one upstream/reference URL is required")
    for url in case.references:
        if not url.startswith("https://"):
            errors.append(f"{prefix}: reference must use https: {url!r}")
    return errors


def validate_all(cases: Iterable[CorpusCase]) -> None:
    cases = list(cases)
    errors: list[str] = []
    ids: set[str] = set()
    commits: dict[tuple[str, str], str] = {}
    for case in cases:
        errors.extend(validate_case(case))
        if case.id in ids:
            errors.append(f"duplicate corpus id {case.id!r}")
        ids.add(case.id)
        key = (case.repository, case.commit)
        # Sharing one commit is allowed (for different build contracts), but record it explicitly.
        commits.setdefault(key, case.id)
    if not any(case.tier == "smoke" for case in cases):
        errors.append("corpus must contain at least one smoke case")
    if not any(case.tier == "extended" for case in cases):
        errors.append("corpus must contain at least one extended case")
    if not any(case.expected_status == "reproducible" for case in cases):
        errors.append("corpus must contain a reproducible negative control")
    if not any(case.expected_status == "non_reproducible" for case in cases):
        errors.append("corpus must contain a non-reproducible case")
    if errors:
        raise SystemExit("real-world corpus validation failed:\n" + "\n".join(f"- {e}" for e in errors))


def render_config(case: CorpusCase, runner: str) -> str:
    text = case.config_path.read_text(encoding="utf-8")
    pattern = re.compile(r'(?m)^runner\s*=\s*"(?:docker|podman)"\s*$')
    if not pattern.search(text):
        raise ValueError(f"{case.id}: config must contain an explicit build.runner for corpus runner substitution")
    rendered, count = pattern.subn(f'runner = "{runner}"', text, count=1)
    if count != 1:
        raise ValueError(f"{case.id}: could not replace build.runner")
    return rendered


def clone_exact(case: CorpusCase, destination: Path) -> None:
    destination.mkdir(parents=True, exist_ok=False)
    run_checked(["git", "init", "-q"], cwd=destination)
    run_checked(["git", "remote", "add", "origin", case.repository], cwd=destination)
    # A full commit SHA is the authority. Fetching exactly one commit avoids branch/tag drift.
    run_checked(["git", "fetch", "--depth=1", "--no-tags", "origin", case.commit], cwd=destination, timeout=300)
    run_checked(["git", "-c", "advice.detachedHead=false", "checkout", "-q", "FETCH_HEAD"], cwd=destination)
    head = run_checked(["git", "rev-parse", "HEAD"], cwd=destination).stdout.strip()
    if head != case.commit:
        raise RuntimeError(f"{case.id}: fetched HEAD {head} != pinned {case.commit}")


def prepare_case(case: CorpusCase, destination: Path, runner: str) -> None:
    if destination.exists():
        raise FileExistsError(f"refusing to overwrite existing corpus checkout: {destination}")
    clone_exact(case, destination)
    config_target = destination / ".reprobisect.toml"
    if config_target.exists() or config_target.is_symlink():
        raise RuntimeError(f"{case.id}: upstream checkout already contains .reprobisect.toml")
    config_target.write_text(render_config(case, runner), encoding="utf-8")
    exclude = destination / ".git" / "info" / "exclude"
    with exclude.open("a", encoding="utf-8") as handle:
        handle.write("\n# ReproBisect real-world corpus overlay\n.reprobisect.toml\n.reprobisect/\n")
    status = run_checked(["git", "status", "--porcelain=v1", "--untracked-files=all"], cwd=destination).stdout
    if status.strip():
        raise RuntimeError(f"{case.id}: corpus overlay dirtied upstream checkout: {status!r}")


def build_helper(case: CorpusCase, runner: str, built: set[str]) -> None:
    if not case.helper_image:
        return
    if case.helper_image in built:
        return
    assert case.helper_path is not None
    run_checked(
        [runner, "build", "--pull", "-t", case.helper_image, "-f", str(case.helper_path), str(IMAGES_ROOT)],
        cwd=ROOT,
        timeout=min(max(case.max_seconds, 600), 1800),
    )
    built.add(case.helper_image)


def diagnosed_variables(report: dict[str, Any]) -> set[str]:
    variables: set[str] = set()
    for diagnosis in report.get("diagnoses", []):
        for variable in diagnosis.get("causal_variables", []):
            if isinstance(variable, str):
                variables.add(variable)
    return variables


def assert_report(case: CorpusCase, report: dict[str, Any], exit_code: int) -> None:
    status = report.get("status")
    if status != case.expected_status:
        raise AssertionError(f"{case.id}: expected status {case.expected_status!r}, got {status!r}")
    expected_exit = 0 if case.expected_status == "reproducible" else 1
    if exit_code != expected_exit:
        raise AssertionError(f"{case.id}: expected exit {expected_exit}, got {exit_code}")
    variables = diagnosed_variables(report)
    missing = sorted(set(case.expected_causal_variables) - variables)
    if missing:
        raise AssertionError(
            f"{case.id}: expected causal variables {missing!r}; diagnosed={sorted(variables)!r}"
        )


def selected_cases(cases: list[CorpusCase], tier: str, ids: list[str]) -> list[CorpusCase]:
    by_id = {case.id: case for case in cases}
    if ids:
        missing = [case_id for case_id in ids if case_id not in by_id]
        if missing:
            raise SystemExit(f"unknown corpus case(s): {', '.join(missing)}")
        return [by_id[case_id] for case_id in ids]
    if tier == "all":
        return cases
    return [case for case in cases if case.tier == tier]


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def run_cases(args: argparse.Namespace, cases: list[CorpusCase]) -> int:
    chosen = selected_cases(cases, args.tier, args.case)
    if not chosen:
        raise SystemExit("no corpus cases selected")
    if args.keep_workdirs and not args.work_root:
        raise SystemExit("--keep-workdirs requires --work-root; temporary roots are always removed")
    binary = Path(args.binary).resolve()
    if not binary.is_file():
        raise SystemExit(f"ReproBisect binary not found: {binary}")
    binary_sha256 = sha256_file(binary)
    built_helpers: set[str] = set()
    results: list[dict[str, Any]] = []
    failures = 0
    if args.work_root:
        work_root = Path(args.work_root).resolve()
        work_root.mkdir(parents=True, exist_ok=True)
        temporary = None
    else:
        temporary = tempfile.TemporaryDirectory(prefix="reprobisect-real-world-")
        work_root = Path(temporary.name)
    try:
        for case in chosen:
            print(f"==> {case.id} [{case.tier}]", flush=True)
            started = time.monotonic()
            result: dict[str, Any] = {
                "id": case.id,
                "tier": case.tier,
                "description": case.description,
                "repository": case.repository,
                "commit": case.commit,
                "upstream_ref": case.upstream_ref,
                "references": list(case.references),
                "case_manifest_sha256": sha256_file(case.directory / "case.toml"),
                "config_sha256": sha256_file(case.config_path),
                "helper_image": case.helper_image,
                "helper_dockerfile_sha256": (
                    sha256_file(case.helper_path) if case.helper_path is not None else None
                ),
                "expected_status": case.expected_status,
                "expected_causal_variables": list(case.expected_causal_variables),
            }
            checkout = work_root / case.id
            try:
                if checkout.exists():
                    if args.replace_checkout:
                        shutil.rmtree(checkout)
                    else:
                        raise FileExistsError(
                            f"work directory already exists: {checkout}; pass --replace-checkout to remove it"
                        )
                if not args.no_helper_build:
                    build_helper(case, args.runner, built_helpers)
                prepare_case(case, checkout, args.runner)
                completed = subprocess.run(
                    [str(binary), "check", str(checkout), "--format", "json"],
                    text=True,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    timeout=case.max_seconds,
                    check=False,
                )
                try:
                    report = json.loads(completed.stdout)
                except json.JSONDecodeError as exc:
                    raise AssertionError(
                        f"{case.id}: stdout was not a JSON report: {exc}; stderr={completed.stderr[-2000:]!r}"
                    ) from exc
                result.update(
                    actual_status=report.get("status"),
                    diagnosed_variables=sorted(diagnosed_variables(report)),
                    exit_code=completed.returncode,
                    experiment_id=report.get("experiment_id"),
                    source_digest=report.get("source_digest"),
                    report=report,
                )
                assert_report(case, report, completed.returncode)
                result["ok"] = True
                print(
                    f"PASS {case.id}: status={report.get('status')} "
                    f"causes={','.join(result['diagnosed_variables']) or '-'}"
                )
            except Exception as exc:  # corpus harness must aggregate failures
                failures += 1
                result.update(ok=False, error=str(exc))
                print(f"FAIL {case.id}: {exc}", file=sys.stderr)
            finally:
                result["duration_seconds"] = round(time.monotonic() - started, 3)
                results.append(result)
                if not args.keep_workdirs and checkout.exists():
                    shutil.rmtree(checkout, ignore_errors=True)
    finally:
        if temporary is not None:
            temporary.cleanup()

    summary = {
        "schema_version": 1,
        "runner": args.runner,
        "reprobisect_binary_sha256": binary_sha256,
        "selected_tier": args.tier,
        "case_count": len(results),
        "passed": sum(1 for item in results if item.get("ok")),
        "failed": failures,
        "results": results,
    }
    if args.json_output:
        output = Path(args.json_output)
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"summary: {summary['passed']}/{summary['case_count']} passed")
    return 1 if failures else 0


def list_cases(cases: list[CorpusCase], as_json: bool) -> None:
    if as_json:
        print(json.dumps([
            {
                "id": case.id,
                "tier": case.tier,
                "repository": case.repository,
                "commit": case.commit,
                "expected_status": case.expected_status,
                "expected_causal_variables": list(case.expected_causal_variables),
                "helper_image": case.helper_image,
            }
            for case in cases
        ], indent=2, sort_keys=True))
        return
    for case in cases:
        causes = ",".join(case.expected_causal_variables) or "-"
        print(f"{case.id:32} {case.tier:8} {case.expected_status:24} {causes}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    validate_p = sub.add_parser("validate", help="validate corpus manifests/configs without network access")
    validate_p.set_defaults(action="validate")
    list_p = sub.add_parser("list", help="list pinned corpus cases")
    list_p.add_argument("--json", action="store_true")
    list_p.set_defaults(action="list")
    prepare_p = sub.add_parser("prepare", help="fetch one pinned upstream checkout and inject the corpus config")
    prepare_p.add_argument("case")
    prepare_p.add_argument("destination")
    prepare_p.add_argument("--runner", choices=sorted(RUNNERS), default="docker")
    prepare_p.set_defaults(action="prepare")
    run_p = sub.add_parser("run", help="fetch and execute selected corpus cases")
    run_p.add_argument("--binary", default=str(ROOT / "target" / "debug" / "reprobisect"))
    run_p.add_argument("--runner", choices=sorted(RUNNERS), default="docker")
    run_p.add_argument("--tier", choices=["smoke", "extended", "all"], default="smoke")
    run_p.add_argument("--case", action="append", default=[], help="specific case id; repeatable; overrides --tier")
    run_p.add_argument("--work-root")
    run_p.add_argument("--keep-workdirs", action="store_true")
    run_p.add_argument(
        "--replace-checkout",
        action="store_true",
        help="remove an existing per-case work directory before fetching the pinned source",
    )
    run_p.add_argument("--no-helper-build", action="store_true")
    run_p.add_argument("--json-output")
    run_p.set_defaults(action="run")
    args = parser.parse_args()

    try:
        cases = load_cases()
        validate_all(cases)
        if args.action == "validate":
            print(f"PASS real-world corpus: {len(cases)} pinned cases ({sum(c.tier == 'smoke' for c in cases)} smoke, {sum(c.tier == 'extended' for c in cases)} extended)")
            return 0
        if args.action == "list":
            list_cases(cases, args.json)
            return 0
        if args.action == "prepare":
            by_id = {case.id: case for case in cases}
            case = by_id.get(args.case)
            if case is None:
                raise SystemExit(f"unknown corpus case: {args.case}")
            prepare_case(case, Path(args.destination).resolve(), args.runner)
            print(f"prepared {case.id} at {Path(args.destination).resolve()}")
            return 0
        if args.action == "run":
            return run_cases(args, cases)
        raise AssertionError(args.action)
    except subprocess.CalledProcessError as exc:
        if exc.stdout:
            print(exc.stdout, file=sys.stderr)
        if exc.stderr:
            print(exc.stderr, file=sys.stderr)
        raise SystemExit(f"command failed ({exc.returncode}): {' '.join(exc.cmd)}") from exc


if __name__ == "__main__":
    raise SystemExit(main())
