import mmap
import os
import sys
from pathlib import Path

dep_fd = int(sys.argv[1])
cache_fd = int(sys.argv[2])

with mmap.mmap(dep_fd, 0, access=mmap.ACCESS_READ) as dep_map:
    dependency = dep_map[:]
with mmap.mmap(cache_fd, 0, access=mmap.ACCESS_READ) as cache_map:
    cache = cache_map[:]

handoff_fd = os.open(
    "build/.handoff.tmp",
    os.O_WRONLY | os.O_CREAT | os.O_TRUNC,
    0o644,
)
try:
    os.write(handoff_fd, dependency + b"|" + cache)
finally:
    os.close(handoff_fd)

Path("build/child.txt").write_bytes(b"child|" + dependency + b"|" + cache)
