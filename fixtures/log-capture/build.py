from __future__ import annotations

import hashlib
import sys
from pathlib import Path

stdout = b"A" * (2 * 1024 * 1024)
stderr = b"B" * (2 * 1024 * 1024)
sys.stdout.buffer.write(stdout)
sys.stderr.buffer.write(stderr)
Path("out.txt").write_text(
    f"{hashlib.sha256(stdout).hexdigest()}\n{hashlib.sha256(stderr).hexdigest()}\n",
    encoding="ascii",
)
