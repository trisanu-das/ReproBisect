import os
import subprocess
import sys
from pathlib import Path

cache_root = Path("/tmp/reprobisect-lineage-private-cache")
cache_root.mkdir(parents=True, exist_ok=True)
cache_blob = cache_root / "blob.bin"
cache_blob.write_bytes(b"cache-lineage-v1")

build = Path("build")
build.mkdir(parents=True, exist_ok=True)

dep_fd = os.open("requirements.txt", os.O_RDONLY)
cache_fd = os.open(str(cache_blob), os.O_RDONLY)
try:
    subprocess.run(
        [sys.executable, "child.py", str(dep_fd), str(cache_fd)],
        pass_fds=(dep_fd, cache_fd),
        check=True,
    )
finally:
    os.close(dep_fd)
    os.close(cache_fd)

os.rename("build/.handoff.tmp", "build/out.txt")
