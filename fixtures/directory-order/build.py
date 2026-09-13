from pathlib import Path
import os

Path("build").mkdir(exist_ok=True)
entries = list(os.scandir("inputs"))
# Deliberately bad build logic: inode allocation is an undeclared materialization-order input.
entries.sort(key=lambda entry: entry.inode())
Path("build/out.txt").write_text("\n".join(entry.name for entry in entries) + "\n", encoding="utf-8")
