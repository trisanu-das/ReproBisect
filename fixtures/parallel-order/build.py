import os
import random
import threading
import time
from pathlib import Path

workers = int(os.environ.get("REPROBISECT_CPU_COUNT", "1"))
Path("build").mkdir(exist_ok=True)

if workers <= 1:
    Path("build/out.txt").write_text("A\nB\nC\nD\n", encoding="utf-8")
else:
    completed = []
    lock = threading.Lock()

    def task(label: str) -> None:
        time.sleep(random.SystemRandom().uniform(0.0, 0.03))
        with lock:
            completed.append(label)

    threads = [threading.Thread(target=task, args=(label,)) for label in "ABCD"]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join()
    Path("build/out.txt").write_text("\n".join(completed) + "\n", encoding="utf-8")
