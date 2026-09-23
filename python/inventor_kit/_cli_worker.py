"""One CLI input per process; native messages never enter the JSON stream."""
from __future__ import annotations

import json
from pathlib import Path
import sys


def execute_drawing(request):
    import hashlib
    import inventor_kit as ik
    from .cli import failure
    path = Path(request['path'])
    result = failure(path, 'execution.not_started', 'Input was not processed')
    result.update(report_type='drawing', units='source_units_unverified', operation='drawing', source_sha256=None,
                  current_state='unverified', qualified=False, snapshot_kind='saved', reference_freshness='unverified',
                  sheets=[], images=[])
    stage = 'inspection'
    try:
        data = ik._file_bytes(path)
        result['source_sha256'] = hashlib.sha256(data).hexdigest()
        info = ik.inspect(data, source_id=str(path))
        result['kind'] = info.metadata.identification.kind
        if result['kind'] != 'drawing':
            result.update(status='unsupported', diagnostics=[dict(code='drawing.kind_unsupported', severity='error',
                message='Drawing options require an identified IDW document.', source=None)])
            return result
        doc = ik.read_drawing(data, source_id=str(path))
        result.update(doc.report(sheet_id=request.get('sheet_id'), details=request.get('drawing_details',False),
                                 list_only=request.get('list_sheets',False)))
        result['stages'][stage] = 'available'
        if request.get('svg'):
            stage = 'export'
            exported = doc.export_svg(request['svg'], sheet_id=request.get('sheet_id'),
                                      allow_partial=request.get('allow_partial',False))
            result['export'] = exported['export']
            result['selected_sheet_id'] = exported['selected_sheet_id']
            result['stages'][stage] = 'available'
    except Exception as error:
        result['status'] = 'error'
        result['stages'][stage] = 'failed'
        result['diagnostics'] = [dict(d) for d in error.diagnostics] if isinstance(error,ik.DrawingDisplayError) else [
            dict(code='input.invalid' if stage=='inspection' else 'export.failed', severity='error',message=str(error),source=None)]
    return result


def execute(request):
    if request.get('operation') == 'drawing':
        return execute_drawing(request)
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
        if kind == 'drawing':
            # A 3D-conversion refusal must not imply that an IDW uses mm.
            report['units'] = 'source_units_unverified'
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
    from .drawing_output import json_bytes
    with Path(sys.argv[1]).open('xb') as stream:
        stream.write(json_bytes(report, (96 if request.get('drawing_details') else 16)*1024*1024))


if __name__ == "__main__":
    main()
