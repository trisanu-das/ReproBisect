#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

cp -R "$repo_root/fixtures/reproducible-c/." "$tmp/"
python3 - "$tmp/.reprobisect.toml" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
text = path.read_text()
text = text.replace('[build]\n', '[build]\nrunner = "podman"\n', 1)
text = text.replace('image = "gcc:14"', 'image = "docker.io/library/gcc:14"', 1)
path.write_text(text)
PY

output="$tmp/report.json"
(
  cd "$repo_root"
  cargo run --locked --quiet -- check "$tmp" --format json > "$output"
)
python3 - "$output" <<'PY'
import json, sys
report = json.load(open(sys.argv[1], encoding='utf-8'))
assert report['status'] == 'reproducible', report['status']
assert len(report['runs']) >= 2
assert {run['runner_backend'] for run in report['runs']} == {'podman'}
print('podman runner smoke test passed')
PY
