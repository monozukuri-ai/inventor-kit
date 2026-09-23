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
    def failed(code, message):
        report = failure(request['path'], code, message)
        if request.get('operation') == 'drawing':
            report.update(report_type='drawing', units='source_units_unverified', source_sha256=None,
                          current_state='unverified', qualified=False, snapshot_kind='saved',
                          reference_freshness='unverified', sheets=[], images=[])
        return report
    with tempfile.TemporaryDirectory(prefix="inventor-cli-") as temporary:
        result_path = Path(temporary) / "result.json"
        output_key = 'svg' if request.get('svg') else 'step'
        destination = Path(request[output_key]) if request.get(output_key) else None
        if destination is not None:
            request = dict(request, **{output_key: str(Path(temporary) / destination.name)})
        with tempfile.TemporaryFile(mode="w+") as log:
            try:
                child = subprocess.run([sys.executable, "-m", "inventor_kit._cli_worker", str(result_path)],
                    input=json.dumps(request), text=True, stdout=log, stderr=log, timeout=timeout, check=False)
            except subprocess.TimeoutExpired:
                return failed("execution.timeout", f"Input exceeded {timeout:g} seconds")
            if child.returncode != 0:
                return failed("execution.worker_failed", f"Conversion process exited with code {child.returncode}")
            max_report = (96 if request.get('drawing_details') else 16) * 1024 * 1024
            if not result_path.is_file() or result_path.stat().st_size > max_report:
                return failed("execution.report_unavailable", "Worker report is missing or exceeds its byte limit")
            try:
                result = json.loads(result_path.read_text(encoding="utf-8"))
            except (OSError, ValueError):
                return failed("execution.report_invalid", "Worker produced an invalid JSON report")
        if destination is not None and result["status"] in ("success", "partial"):
            # Publish only after normal native-process shutdown. A timeout or
            # crash can leave incomplete staged files, never final outputs.
            staged = Path(request[output_key])
            created = []
            try:
                # The worker knew only its staging path. Record the final name
                # before publishing a drawing's provenance sidecar.
                if output_key == 'svg':
                    from .drawing_output import json_bytes
                    sidecar = staged.with_suffix(staged.suffix + '.json')
                    value = json.loads(sidecar.read_text(encoding='utf-8'))
                    value['export']['path'] = str(destination)
                    sidecar.write_bytes(json_bytes(value))
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
    parser = Parser(description="Inspect saved Inventor parts, assemblies and drawings")
    parser.add_argument("paths", nargs="+")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--metadata-only", action="store_true")
    mode.add_argument("--list-candidates", action="store_true")
    mode.add_argument("--list-bodies", action="store_true", help="Report every saved body's conversion result")
    mode.add_argument("--convert", action="store_true", help="Report IPT bodies or saved IAM occurrences")
    mode.add_argument('--list-sheets', action='store_true', help='List saved IDW sheets and input-bound IDs')
    mode.add_argument('--drawing-report', action='store_true', help='Report IDW views, raw text runs, sources and omissions')
    output = parser.add_mutually_exclusive_group()
    output.add_argument("--step", type=Path, help="Export a single input to a new STEP and JSON sidecar")
    output.add_argument("--output-dir", type=Path, help="Export each input to a new STEP in this directory")
    output.add_argument('--svg', type=Path, help='Save one IDW sheet as a new offline SVG and JSON sidecar')
    parser.add_argument('--sheet-id', help='Select an ID from --list-sheets; required for multi-sheet SVG output')
    parser.add_argument('--drawing-details', action='store_true', help='Include saved item geometry in the drawing report')
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
    drawing = bool(args.list_sheets or args.drawing_report or args.svg)
    if drawing and (args.metadata_only or args.list_candidates or args.list_bodies or args.convert or args.step or args.output_dir
                    or args.candidate_id or args.body_id or args.search_root or args.allow_unverified_state or args.require_current_state):
        parser.error('Drawing options cannot be combined with metadata, IPT/IAM or STEP options')
    if not drawing and (args.sheet_id or args.drawing_details):
        parser.error('--sheet-id/--drawing-details require --drawing-report or --svg')
    if args.list_sheets and (args.svg or args.sheet_id or args.drawing_details):
        parser.error('--list-sheets cannot be combined with selection, details or SVG output')
    if (args.svg or args.sheet_id) and len(args.paths) != 1:
        parser.error('SVG output and sheet IDs apply to one input')
    if args.svg and args.svg.suffix.lower() != '.svg':
        parser.error('--svg output must use .svg')
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
    if not converting and not drawing and (args.allow_partial or args.allow_unverified_state or args.search_root):
        parser.error("Assembly and partial options require --convert or STEP output")
    paths = [Path(p).resolve() for p in args.paths]
    outputs = [args.svg.resolve() if args.svg else args.step.resolve() if args.step else (args.output_dir.resolve() / (p.stem + ".step")
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
    legacy = len(paths) == 1 and not (drawing or args.convert or args.list_bodies or args.body_id or args.output_dir
        or args.jsonl or args.report or args.allow_partial or args.allow_unverified_state or args.search_root)
    for path, output_path in zip(paths, outputs):
        request = dict(path=str(path), operation='drawing' if drawing else "convert" if converting else "inspect",
                       metadata_only=args.metadata_only, list_candidates=args.list_candidates,
                       list_bodies=args.list_bodies, candidate_id=args.candidate_id,
                       require_current_state=args.require_current_state, body_ids=args.body_id,
                       search_roots=[str(Path(p).resolve()) for p in args.search_root],
                       allow_unverified_state=args.allow_unverified_state, allow_partial=args.allow_partial,
                       step=str(output_path) if output_path and not drawing else None, legacy_summary=legacy,
                       svg=str(output_path) if output_path and drawing else None, sheet_id=args.sheet_id,
                       list_sheets=args.list_sheets, drawing_details=args.drawing_details)
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
