"""Pinned regression for partial topology and nested subtype provenance.

Evaluator agreement uses the same decoded poles. It is not a vendor oracle.
"""
import argparse
from collections import Counter
from dataclasses import asdict
import hashlib
import json
from pathlib import Path

import cq_acis as acis
import inventor_kit
from corpus_manifest import load_manifest

ROOT = Path(__file__).resolve().parents[1]
NAME = 'INV_nist_ftc_07_asme1_2021.ipt'


def validate():
    item = next(e for e in load_manifest(split='regression') if e['file'] == NAME)
    data = (ROOT/'fixtures/public'/NAME).read_bytes()
    assert len(data) == item['bytes'] and hashlib.sha256(data).hexdigest() == item['sha256']
    doc = inventor_kit.read(data, source_id=NAME)
    model = doc.model
    native = acis.NativeModel.from_model(model)
    table = native.subtype_table
    assert (len(table.definitions), len(table.references), len(table.diagnostics)) == (430,225,0)
    views, unsupported, aliases = [], [], []
    converter = acis.CadQueryConverter(model)
    placement = converter._body_placement(model.bodies()[0])
    carrier = doc.summary['carrier']

    def provenance(raw):
        span = raw.source
        assert span.source_id == carrier['source_id']
        assert raw.raw_data == doc.kernel_bytes[
            span.start_offset-carrier['kernel_offset']:span.end_offset-carrier['kernel_offset']]
        return {'entity':raw.index, 'type':raw.type_name,
                'raw_sha256':hashlib.sha256(raw.raw_data).hexdigest(), 'source':asdict(span)}

    for entity in model.entities:
        raw = entity if isinstance(entity, acis.RawEntity) else entity.raw
        if raw.type_name in ('tvertex-vertex','tedge-edge','tcoedge-coedge'):
            entry = provenance(raw)
            try:
                view = native.tolerant_topology(raw.index)
            except acis.AcisModelError as error:
                assert raw.type_name == 'tcoedge-coedge' and raw.values[12] == 1
                unsupported.append(dict(entry,error=str(error),reason='inline coedge curve remains opaque'))
            else:
                entry['view'] = type(view).__name__
                if isinstance(view,acis.TolerantVertex):
                    assert view.vertex.raw == raw
                    entry.update(source_flag=view.source_flag,saved_scalars=view.saved_scalars)
                elif isinstance(view,acis.TolerantEdge):
                    assert view.edge.raw == raw
                    entry.update(saved_scalar=view.saved_scalar,embedded_version=view.embedded_version)
                else:
                    assert isinstance(view,acis.TolerantCoedge) and view.coedge.raw == raw
                    entry.update(parameter_interval=view.parameter_interval,attachment=view.attachment.index)
                assert isinstance(model.resolve(raw.index),acis.RawEntity)
                views.append(entry)
        if raw.type_name not in ('intcurve-curve','spline-surface'):
            continue
        try:
            resolved = native.resolve_subtype(raw.index)
        except acis.AcisModelError as error:
            unsupported.append(dict(provenance(raw),error=str(error),reason='unqualified subtype geometry envelope'))
            continue
        if resolved is None:
            continue
        geometry, definition = resolved.geometry, resolved.definition
        assert geometry.raw == raw
        assert isinstance(geometry,acis.BSplineSurfaceEntity)
        owner = model.resolve(definition.entity_index)
        owner = owner if isinstance(owner,acis.RawEntity) else owner.raw
        assert owner.values[definition.value_start] == b'\x0f'
        assert owner.values[definition.value_end-1] == b'\x10'
        assert owner.values[definition.value_start+1] == definition.kind
        assert definition.parent is not None  # All 32 are nested in saved pcurves.
        geometry.validate()
        occt = converter._bspline_geometry(geometry,placement)
        max_deviation = 0.
        for u in [geometry.u_knots[0],sum((geometry.u_knots[0],geometry.u_knots[-1]))/2,geometry.u_knots[-1]]:
            for v in [geometry.v_knots[0],sum((geometry.v_knots[0],geometry.v_knots[-1]))/2,geometry.v_knots[-1]]:
                p = placement.point_vector(geometry.evaluate(u,v))
                q = occt.Value(u,v)
                deviation = (p-acis.Vec3(q.X(),q.Y(),q.Z())).magnitude
                assert deviation <= converter.tolerance
                max_deviation = max(max_deviation,deviation)
        aliases.append({'reference':provenance(raw), 'definition':asdict(definition),
                        'definition_record':provenance(owner),
                        'same_poles_evaluator_max_deviation_mm':max_deviation})
    counts = dict(Counter(e['view'] for e in views))
    assert counts == {'TolerantVertex':136,'TolerantEdge':103,'TolerantCoedge':196}
    assert len(aliases) == 32 and len(unsupported) == 16
    return {'file':NAME, 'sha256':item['sha256'], 'split':'regression',
            'scope':'partial topology fields and subtype provenance; no vendor or current-state oracle',
            'save_version':22700, 'table':asdict(table), 'view_counts':counts,
            'views':views, 'resolved_surfaces':aliases, 'unsupported':unsupported,
            'surface_samples_each':9, 'source_tolerance_mm':converter.tolerance}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT/'internal/reports/latest/topology.json')
    args = parser.parse_args()
    report = validate()
    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({'views':report['view_counts'], 'resolved_surfaces':len(report['resolved_surfaces']),
                      'unsupported':len(report['unsupported'])}))


if __name__ == '__main__':
    main()
