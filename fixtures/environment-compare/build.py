import os
from pathlib import Path

a = os.environ.get("A", "")
b = os.environ.get("B", "")
# NOISE intentionally does not affect the artifact.
payload = "bad\n" if a == "bad" and b == "bad" else "good\n"
Path("out.txt").write_text(payload, encoding="utf-8")
