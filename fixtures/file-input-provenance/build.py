from pathlib import Path

cache = Path("/tmp/reprobisect-private-cache")
cache.mkdir(parents=True, exist_ok=True)
blob = cache / "blob.bin"
blob.write_bytes(b"cache-input-v1")
requirements = Path("requirements.txt").read_bytes()
cache_bytes = blob.read_bytes()
out = Path("build/out.txt")
out.parent.mkdir(parents=True, exist_ok=True)
out.write_bytes(requirements + b"|" + cache_bytes)
