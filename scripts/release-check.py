#!/usr/bin/env python3
"""Release-policy gate for ReproBisect.

The default mode is intentionally useful in restricted/offline source-review
sandboxes: it validates immutable source policy and runs all local non-Rust
checks. --require-rust upgrades missing rustc/cargo from SKIP to failure and
executes the compiler/unit gates used by the release workflow.
"""

from __future__ import annotations

import argparse
import json
import re
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
EXPECTED_VERSION = "1.1.0-rc.1"
EXPECTED_RELEASE_BRANCH = "develop/1.1.0"
EXPECTED_TAG_GLOB = "v1.1.*"
EXPECTED_RUST_VERSION = "1.85"
EXPECTED_TOOLCHAIN = "1.85.1"
EXPECTED_SCHEMAS = {
    "CHECK_REPORT_SCHEMA_MIN": 5,
    "CHECK_REPORT_SCHEMA_CURRENT": 14,
    "BUILD_EVIDENCE_SCHEMA_MIN": 1,
    "BUILD_EVIDENCE_SCHEMA_CURRENT": 10,
    "FIX_REPORT_SCHEMA_MIN": 2,
    "FIX_REPORT_SCHEMA_CURRENT": 12,
    "ENVIRONMENT_COMPARISON_SCHEMA_MIN": 1,
    "ENVIRONMENT_COMPARISON_SCHEMA_CURRENT": 4,
}
ACTION_SHA_RE = re.compile(r"^[0-9a-f]{40}$")


def run(command: list[str], *, required: bool = True) -> dict[str, Any]:
    completed = subprocess.run(
        command,
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    result = {
        "command": command,
        "returncode": completed.returncode,
        "stdout_tail": completed.stdout[-4000:],
        "stderr_tail": completed.stderr[-4000:],
        "ok": completed.returncode == 0,
    }
    if required and completed.returncode != 0:
        message = completed.stderr.strip() or completed.stdout.strip()
        raise RuntimeError(f"command failed ({completed.returncode}): {' '.join(command)}\n{message}")
    return result


def check_package() -> list[str]:
    notes: list[str] = []
    cargo = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    package = cargo["package"]
    if package.get("version") != EXPECTED_VERSION:
        raise RuntimeError(f"Cargo.toml version must be {EXPECTED_VERSION}")
    if package.get("edition") != "2024":
        raise RuntimeError("Cargo.toml edition must remain 2024")
    if package.get("rust-version") != EXPECTED_RUST_VERSION:
        raise RuntimeError(f"Cargo.toml rust-version must remain {EXPECTED_RUST_VERSION}")
    if package.get("publish") is not False:
        raise RuntimeError("release candidate must remain publish = false until crates.io policy is decided")

    toolchain = tomllib.loads((ROOT / "rust-toolchain.toml").read_text(encoding="utf-8"))
    if toolchain.get("toolchain", {}).get("channel") != EXPECTED_TOOLCHAIN:
        raise RuntimeError(f"rust-toolchain.toml must pin {EXPECTED_TOOLCHAIN}")

    lock_path = ROOT / "Cargo.lock"
    if not lock_path.is_file():
        raise RuntimeError("Cargo.lock must be committed for the stable release line")
    lock = tomllib.loads(lock_path.read_text(encoding="utf-8"))
    root_packages = [
        package for package in lock.get("package", []) if package.get("name") == "reprobisect"
    ]
    if len(root_packages) != 1 or root_packages[0].get("version") != EXPECTED_VERSION:
        raise RuntimeError(f"Cargo.lock must contain reprobisect {EXPECTED_VERSION}")
    notes.append("Cargo.lock is committed and matches the package version")
    notes.append(f"package={EXPECTED_VERSION}; msrv={EXPECTED_RUST_VERSION}; default_toolchain={EXPECTED_TOOLCHAIN}")
    return notes


def check_schemas() -> list[str]:
    text = (ROOT / "src" / "schema.rs").read_text(encoding="utf-8")
    found = {name: int(value) for name, value in re.findall(r"pub const ([A-Z_]+): u32 = (\d+);", text)}
    for name, value in EXPECTED_SCHEMAS.items():
        if found.get(name) != value:
            raise RuntimeError(f"schema freeze violation: {name}={found.get(name)!r}, expected {value}")
    return ["persisted evidence schema ranges remain unchanged from the 1.0 stable line into 1.1"]


def check_action_pins() -> list[str]:
    checked = 0
    for path in sorted((ROOT / ".github" / "workflows").glob("*.yml")):
        for lineno, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            match = re.search(r"\buses:\s*([^\s#]+)", line)
            if not match:
                continue
            value = match.group(1)
            if value.startswith("./"):
                continue
            if "@" not in value:
                raise RuntimeError(f"{path.relative_to(ROOT)}:{lineno}: action ref lacks @commit")
            _, ref = value.rsplit("@", 1)
            if not ACTION_SHA_RE.fullmatch(ref):
                raise RuntimeError(
                    f"{path.relative_to(ROOT)}:{lineno}: action must be pinned to a 40-hex commit, got {ref!r}"
                )
            checked += 1
    if checked == 0:
        raise RuntimeError("no external GitHub Actions references were checked")
    return [f"{checked} external GitHub Actions references are commit-pinned"]


def check_release_workflow() -> list[str]:
    path = ROOT / ".github" / "workflows" / "release.yml"
    text = path.read_text(encoding="utf-8")
    required = [
        f"- '{EXPECTED_RELEASE_BRANCH}'",
        f"- '{EXPECTED_TAG_GLOB}'",
        "id: release_meta",
        "steps.release_meta.outputs.version",
        "Verify release tag matches package version",
        "cancel-in-progress: true",
    ]
    missing = [value for value in required if value not in text]
    if missing:
        raise RuntimeError(
            "release workflow is missing 1.1 candidate safeguards: " + ", ".join(missing)
        )
    if "reprobisect 1.0.0" in text or 'version="1.0.0"' in text:
        raise RuntimeError("release workflow still hardcodes the 1.0.0 package version")
    return [
        f"release workflow targets {EXPECTED_RELEASE_BRANCH} / {EXPECTED_TAG_GLOB} and derives packaging version from Cargo.toml"
    ]


def check_required_files() -> list[str]:
    required = [
        "LICENSE",
        "README.md",
        "SECURITY.md",
        "CHANGELOG.md",
        "docs/release-qualification.md",
        "docs/evidence-compatibility.md",
        "docs/real-world-validation.md",
    ]
    missing = [name for name in required if not (ROOT / name).is_file()]
    if missing:
        raise RuntimeError("missing release files: " + ", ".join(missing))
    return ["release/security/compatibility documentation present"]


def source_checks() -> tuple[list[str], list[dict[str, Any]]]:
    notes: list[str] = []
    commands: list[dict[str, Any]] = []
    notes += check_package()
    notes += check_schemas()
    notes += check_action_pins()
    notes += check_release_workflow()
    notes += check_required_files()
    commands.append(run([sys.executable, "scripts/static-check.py"]))
    commands.append(run([sys.executable, "scripts/real-world-corpus.py", "validate"]))
    commands.append(run(["bash", "-n", "scripts/test-fixtures.sh"]))
    commands.append(run(["bash", "-n", "scripts/test-podman.sh"]))
    if shutil.which("git") and (ROOT / ".git").exists():
        commands.append(run(["git", "diff", "--check"]))
    return notes, commands


def rust_checks(require_rust: bool) -> tuple[list[str], list[dict[str, Any]]]:
    cargo = shutil.which("cargo")
    rustc = shutil.which("rustc")
    if not cargo or not rustc:
        if require_rust:
            raise RuntimeError("rustc/cargo are required for full release qualification but are unavailable")
        return ["SKIP: rustc/cargo unavailable; compiler/unit gates not executed"], []
    commands = [
        run([rustc, "--version"]),
        run([cargo, "--version"]),
        run([cargo, "check", "--locked", "--all-targets"]),
        run([cargo, "test", "--locked", "--all-targets"]),
    ]
    return ["Rust compiler and all-target unit gates passed"], commands


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-only", action="store_true", help="do not attempt compiler/unit checks")
    parser.add_argument("--require-rust", action="store_true", help="fail if rustc/cargo are unavailable")
    parser.add_argument("--json-output", help="write a machine-readable qualification record")
    args = parser.parse_args()
    if args.source_only and args.require_rust:
        parser.error("--source-only and --require-rust are mutually exclusive")

    record: dict[str, Any] = {
        "schema_version": 1,
        "release_candidate": EXPECTED_VERSION,
        "source_policy": "pending",
        "rust_qualification": "not_requested" if args.source_only else "pending",
        "notes": [],
        "commands": [],
    }
    try:
        notes, commands = source_checks()
        record["notes"].extend(notes)
        record["commands"].extend(commands)
        record["source_policy"] = "passed"
        if not args.source_only:
            notes, commands = rust_checks(args.require_rust)
            record["notes"].extend(notes)
            record["commands"].extend(commands)
            record["rust_qualification"] = "passed" if commands else "skipped"
        record["ok"] = True
    except Exception as exc:
        record["ok"] = False
        record["error"] = str(exc)

    if args.json_output:
        path = Path(args.json_output)
        if not path.is_absolute():
            path = ROOT / path
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(record, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    if record["ok"]:
        print(f"PASS release source policy for {EXPECTED_VERSION}")
        for note in record["notes"]:
            print(f"- {note}")
        return 0
    print(f"FAIL release qualification: {record.get('error')}", file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
