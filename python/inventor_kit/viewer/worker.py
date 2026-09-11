"""A spawned worker publishes metadata early; final geometry requires exit 0."""
import json
import multiprocessing
from pathlib import Path
import time

from .scene import build_scene, diagnostic, discard_geometry, empty_scene, write_scene


def run_worker(path, directory, options):
    directory = Path(directory)
    scene = build_scene(path, directory, options, lambda s: write_scene(directory, s))
    write_scene(directory, scene, "pending.json")


class Job:
    def __init__(self, path, directory, options, target=run_worker):
        self.directory = Path(directory)
        self.options = options
        self.done = False
        write_scene(self.directory, empty_scene(Path(path).name))
        self.process = multiprocessing.get_context("spawn").Process(
            target=target, args=(str(path), str(directory), options))
        self.process.start()
        self.started = time.monotonic()

    def poll(self):
        if self.done:
            return
        timeout = time.monotonic() - self.started > self.options.timeout
        if self.process.is_alive() and not timeout:
            return
        if self.process.is_alive():
            self.stop()
        else:
            self.process.join()
        pending = self.directory / "pending.json"
        if not timeout and self.process.exitcode == 0 and pending.exists():
            pending.replace(self.directory / "state.json")
        else:
            scene = json.loads((self.directory / "state.json").read_text(encoding="utf-8"))
            scene["job_status"] = "failed"
            discard_geometry(scene, "worker_failed", "Conversion process did not complete")
            diagnostic(scene, "viewer.timeout" if timeout else "viewer.worker_failed",
                       "Conversion exceeded its time limit." if timeout else f"Conversion process exited with code {self.process.exitcode}.")
            write_scene(self.directory, scene)
        self.done = True

    def stop(self):
        if self.process.is_alive():
            self.process.terminate()
            self.process.join(2)
        if self.process.is_alive():
            self.process.kill()
        self.process.join()
