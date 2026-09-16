"""Isolated per-input jobs and machine-readable conversion reports."""
from __future__ import annotations

import argparse
import json
import math
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


class Parser(argparse.ArgumentParser):
    def error(self, message):
        self.print_usage(sys.stderr)
        self.exit(1, f"{self.prog}: error: {message}\n")


def failure(path, code, message):
    return dict(schema_version=1, kind="unknown", input=Path(path).name, status="error",
                units="mm", complete=False, current_state_verified=False,
                geometry_complete=False, selection_complete=False,
                stages=dict(inspection="not_attempted", conversion="not_attempted", export="not_attempted"),
                diagnostics=[dict(code=code, severity="error", message=message, source=None)])


def run_job(request, timeout):
    with tempfile.TemporaryDirectory(prefix="inventor-cli-") as temporary:
        result_path = Path(temporary) / "result.json"
        destination = Path(request["step"]) if request.get("step") else None
        if destination is not None:
            request = dict(request, step=str(Path(temporary) / destination.name))
        with tempfile.TemporaryFile(mode="w+") as log:
            try:
                child = subprocess.run([sys.executable, "-m", "inventor_kit._cli_worker", str(result_path)],
                    input=json.dumps(request), text=True, stdout=log, stderr=log, timeout=timeout, check=False)
            except subprocess.TimeoutExpired:
                return failure(request["path"], "execution.timeout", f"Input exceeded {timeout:g} seconds")
            if child.returncode != 0:
                return failure(request["path"], "execution.worker_failed", f"Conversion process exited with code {child.returncode}")
            if not result_path.is_file() or result_path.stat().st_size > 16 * 1024 * 1024:
                return failure(request["path"], "execution.report_unavailable", "Worker report is missing or exceeds 16 MiB")
            try:
                result = json.loads(result_path.read_text(encoding="utf-8"))
            except (OSError, ValueError):
                return failure(request["path"], "execution.report_invalid", "Worker produced an invalid JSON report")
        if destination is not None and result["status"] in ("success", "partial"):
            # Publish only after normal native-process shutdown. A timeout or
            # crash can leave incomplete staged files, never final outputs.
            staged = Path(request["step"])
            created = []
            try:
                for source, target in ((staged, destination),
                        (staged.with_suffix(staged.suffix + ".json"), destination.with_suffix(destination.suffix + ".json"))):
                    with source.open("rb") as incoming, target.open("xb") as outgoing:
                        created.append(target)
                        shutil.copyfileobj(incoming, outgoing)
                result["export"]["path"] = str(destination)
            except BaseException as error:
                for target in created:
                    target.unlink()
                if not isinstance(error, Exception):
                    raise
                result["status"] = "error"
                result["stages"]["export"] = "failed"
                result.pop("export", None)
                result["diagnostics"].append(dict(code="export.failed", severity="error", message=str(error), source=None))
        return result


def main(argv=None):
    parser = Parser(description="Inspect and convert saved Inventor parts and assemblies")
    parser.add_argument("paths", nargs="+")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--metadata-only", action="store_true")
    mode.add_argument("--list-candidates", action="store_true")
    mode.add_argument("--list-bodies", action="store_true", help="Report every saved body's conversion result")
    mode.add_argument("--convert", action="store_true", help="Report IPT bodies or saved IAM occurrences")
    output = parser.add_mutually_exclusive_group()
    output.add_argument("--step", type=Path, help="Export a single input to a new STEP and JSON sidecar")
    output.add_argument("--output-dir", type=Path, help="Export each input to a new STEP in this directory")
    parser.add_argument("--report", type=Path, help="Save the JSON result to a new file")
    parser.add_argument("--jsonl", action="store_true", help="One JSON object per input on stdout and in --report")
    parser.add_argument("--candidate-id")
    parser.add_argument("--require-current-state", action="store_true")
    parser.add_argument("--body-id", action="append", default=[])
    parser.add_argument("--search-root", action="append", default=[])
    parser.add_argument("--allow-unverified-state", action="store_true")
    parser.add_argument("--allow-partial", action="store_true")
    parser.add_argument("--timeout", type=float, default=120, help="Seconds per input, including native conversion")
    args = parser.parse_args(argv)
    if not math.isfinite(args.timeout) or args.timeout <= 0:
        parser.error("--timeout must be finite and positive")
    if args.step and len(args.paths) != 1:
        parser.error("--step accepts one input; use --output-dir for batches")
    if (args.body_id or args.candidate_id) and len(args.paths) != 1:
        parser.error("Candidate and body IDs are tied to one input")
    inspection = args.metadata_only or args.list_candidates
    if inspection and (args.step or args.output_dir or args.candidate_id or args.body_id or
                       args.require_current_state or args.allow_partial or args.allow_unverified_state or args.search_root):
        parser.error("Geometry and assembly options cannot be combined with metadata/candidate inspection")
    if args.list_bodies and (args.step or args.output_dir):
        parser.error("--list-bodies cannot be combined with STEP output")
    converting = bool(args.convert or args.list_bodies or args.step or args.output_dir or args.body_id)
    if not converting and (args.allow_partial or args.allow_unverified_state or args.search_root):
        parser.error("Assembly and partial options require --convert or STEP output")
    paths = [Path(p).resolve() for p in args.paths]
    outputs = [args.step.resolve() if args.step else (args.output_dir.resolve() / (p.stem + ".step")
               if args.output_dir else None) for p in paths]
    artifacts = [p for output in outputs if output for p in (output, output.with_suffix(output.suffix + ".json"))]
    if args.report:
        artifacts.append(args.report.resolve())
    if len({str(p).casefold() for p in artifacts}) != len(artifacts):
        parser.error("Output names collide; use separate directories or distinct input names")
    if any(p in paths or p.exists() for p in artifacts):
        parser.error("Outputs must be new files and must not replace any input")
    if args.output_dir:
        try:
            args.output_dir.mkdir(parents=True, exist_ok=True)
        except OSError as error:
            parser.error(f"Could not create output directory: {error}")
    results = []
    legacy = len(paths) == 1 and not (args.convert or args.list_bodies or args.body_id or args.output_dir
        or args.jsonl or args.report or args.allow_partial or args.allow_unverified_state or args.search_root)
    for path, output_path in zip(paths, outputs):
        request = dict(path=str(path), operation="convert" if converting else "inspect",
                       metadata_only=args.metadata_only, list_candidates=args.list_candidates,
                       list_bodies=args.list_bodies, candidate_id=args.candidate_id,
                       require_current_state=args.require_current_state, body_ids=args.body_id,
                       search_roots=[str(Path(p).resolve()) for p in args.search_root],
                       allow_unverified_state=args.allow_unverified_state, allow_partial=args.allow_partial,
                       step=str(output_path) if output_path else None, legacy_summary=legacy)
        result = run_job(request, args.timeout)
        results.append(result)
        if args.jsonl:
            print(json.dumps(result, ensure_ascii=False, allow_nan=False), flush=True)
    value = results[0] if len(results) == 1 else results
    if not args.jsonl:
        print(json.dumps(results[0].get("document", value) if legacy and results[0]["status"] != "error" else value,
                         ensure_ascii=False, indent=2, allow_nan=False))
    if args.report:
        data = ("".join(json.dumps(r, ensure_ascii=False, allow_nan=False) + "\n" for r in results)
                if args.jsonl else json.dumps(value, ensure_ascii=False, indent=2, allow_nan=False) + "\n")
        try:
            with args.report.open("x", encoding="utf-8") as stream:
                stream.write(data)
        except OSError as error:
            print(f"Could not write report: {error}", file=sys.stderr)
            return 1
    statuses = {r["status"] for r in results}
    if legacy and not converting and "document" in results[0]:
        return 0  # Original inspect CLI reports state/profile refusal in its summary.
    return 1 if "error" in statuses else 3 if "unsupported" in statuses else 2 if "partial" in statuses else 0
