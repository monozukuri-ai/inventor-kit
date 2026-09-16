"""One CLI input per process; native messages never enter the JSON stream."""
from __future__ import annotations

import json
from pathlib import Path
import sys


def execute(request):
    import inventor_kit as ik
    from .cli import failure
    from .conversion import dependency_versions
    path = Path(request["path"])
    report = failure(path, "execution.not_started", "Input was not processed")
    report["operation"] = request["operation"]
    stages = report["stages"]
    stage = "inspection"
    try:
        doc = ik.inspect_file(path, include_candidates=request["list_candidates"])
        kind = doc.metadata.identification.kind
        report.update(kind=kind, diagnostics=[], dependencies=dependency_versions(),
                      source_sha256=doc.geometry.source_sha256)
        stages[stage] = "available"
        if request["operation"] == "inspect":
            if not (request["metadata_only"] or request["list_candidates"]):
                doc = ik.read_file(path, candidate_id=request["candidate_id"],
                                   require_current_state=request["require_current_state"])
            report.update(document=doc.summary, status="success" if
                          (request["metadata_only"] or request["list_candidates"] or doc.model is not None)
                          else "unsupported")
            return report
        stage = "conversion"
        if kind == "part":
            if request["search_roots"] or request["allow_unverified_state"]:
                raise ValueError("Assembly options apply to IAM inputs only")
            doc = ik.read_file(path, candidate_id=request["candidate_id"],
                               require_current_state=request["require_current_state"])
            result = doc.convert_bodies()
            report.update(result.report(body_ids=request["body_ids"] or None))
            if request.get("legacy_summary"):
                report["document"] = doc.summary
            stages[stage] = report["status"]
            if request["step"]:
                stage = "export"
                step = result.export_step(request["step"], body_ids=request["body_ids"] or None,
                                          allow_partial=request["allow_partial"])
                report["export"] = dict(path=request["step"], sha256=step["step_sha256"], roundtrip=step["roundtrip"])
                stages[stage] = "available"
        elif kind == "assembly":
            if request["candidate_id"] or request["body_ids"] or request["require_current_state"] or request["list_bodies"]:
                raise ValueError("Candidate, body and current-state options apply to IPT inputs only")
            graph = ik.read_assembly_file(path, search_roots=request["search_roots"])
            try:
                result = graph.to_cadquery(allow_unverified_state=request["allow_unverified_state"],
                                          allow_partial=request["allow_partial"])
            except ik.AssemblyConversionError as error:
                report.update(error.result.report())
                raise
            report.update(result.report())
            stages[stage] = report["status"]
            if request["step"]:
                stage = "export"
                step = result.export_step(request["step"], allow_partial=request["allow_partial"])
                report["export"] = dict(path=request["step"], sha256=step["step_sha256"], roundtrip=step["roundtrip"])
                stages[stage] = "available"
        else:
            report.update(status="unsupported", diagnostics=[dict(code="document.kind_unsupported", severity="error",
                message="Saved geometry conversion supports IPT and IAM inputs", source=None)])
            stages[stage] = "unsupported"
    except Exception as error:
        report["status"] = "error"
        stages[stage] = "failed"
        report["diagnostics"].append(dict(code=getattr(error, "code", "input.invalid" if stage == "inspection" else
            "conversion.selection_refused" if isinstance(error, (ik.BodyConversionError, ik.AssemblyConversionError))
            else f"{stage}.failed"), severity="error", message=str(error), source=None))
    return report


def main():
    request = json.load(sys.stdin)
    report = execute(request)
    with Path(sys.argv[1]).open("x", encoding="utf-8") as stream:
        json.dump(report, stream, ensure_ascii=False, allow_nan=False)


if __name__ == "__main__":
    main()
