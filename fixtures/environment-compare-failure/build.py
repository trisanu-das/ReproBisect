import os
import sys
from pathlib import Path

a = os.environ.get("A", "")
b = os.environ.get("B", "")
# NOISE is deliberately irrelevant to the observed outcome.
if a == "bad" and b == "bad":
    print("controlled known-bad failure", file=sys.stderr)
    raise SystemExit(37)

Path("out.txt").write_text("good\n", encoding="utf-8")
