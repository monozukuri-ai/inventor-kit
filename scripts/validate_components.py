"""Component checks for the bounded explicit ASM spline profile.

These compare source vertices and two evaluators of the same decoded poles.
They do not establish agreement with Inventor's current state or a vendor B-rep.
"""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path

import cq_acis as acis
import inventor_kit
from corpus_manifest import load_manifest

ROOT = Path(__file__).resolve().parents[1]
NAME = 'INV_nist_ftc_07_asme1_2021.ipt'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT/'internal/reports/latest/components.json')
    args = parser.parse_args()
    item = next(e for e in load_manifest(split='regression') if e['file'] == NAME)
    data = (ROOT/'fixtures/public'/NAME).read_bytes()
    assert len(data) == item['bytes'] and hashlib.sha256(data).hexdigest() == item['sha256']
    doc = inventor_kit.read(data, source_id=NAME)
    model = doc.model
    assert len(model.bodies()) == 1
    converter = acis.CadQueryConverter(model)
    placement = converter._body_placement(model.bodies()[0])
    carrier = doc.summary['carrier']
    curves, surfaces, trims = [], [], []
    max_surface_deviation = 0.
    for entity in model.entities:
        if not isinstance(entity, (acis.BSplineCurveEntity, acis.BSplineSurfaceEntity)):
            continue
        entity.validate()
        raw = entity.raw
        span = raw.source
        assert span.source_id == carrier['source_id']
        assert raw.raw_data == doc.kernel_bytes[
            span.start_offset-carrier['kernel_offset']:span.end_offset-carrier['kernel_offset']]
        entry = {'entity': entity.index, 'raw_sha256': hashlib.sha256(raw.raw_data).hexdigest(),
                 'source_id': span.source_id, 'start_offset': span.start_offset, 'end_offset': span.end_offset}
        if isinstance(entity, acis.BSplineCurveEntity):
            curves.append(entry)
        else:
            occt = converter._bspline_geometry(entity, placement)
            for u in [entity.u_knots[0], sum((entity.u_knots[0],entity.u_knots[-1]))/2, entity.u_knots[-1]]:
                for v in [entity.v_knots[0], sum((entity.v_knots[0],entity.v_knots[-1]))/2, entity.v_knots[-1]]:
                    p = placement.point_vector(entity.evaluate(u, v))
                    q = occt.Value(u,v)
                    deviation = (p-acis.Vec3(q.X(),q.Y(),q.Z())).magnitude
                    assert deviation <= converter.tolerance
                    max_surface_deviation = max(max_surface_deviation,deviation)
            surfaces.append(entry)
    for coedge in model.entities:
        if not isinstance(coedge, acis.CoedgeEntity):
            continue
        edge = model.resolve(coedge.edge)
        if not isinstance(edge, acis.EdgeEntity) or not isinstance(model.resolve(edge.curve), acis.BSplineCurveEntity):
            continue
        entry = {'coedge': coedge.index, 'edge': edge.index, 'curve': edge.curve.index}
        try:
            shape = converter._edge(coedge,placement)
            assert shape.isValid()
            entry.update(status='converted_source_endpoints_checked')
        except acis.CadQueryConversionError as error:
            entry.update(status='unsupported', error_code=error.code, error=str(error))
        trims.append(entry)
    counts = dict(Counter(e['status'] for e in trims))
    assert (len(curves),len(surfaces),counts.get('converted_source_endpoints_checked')) == (64,16,88)
    assert not any(e['status'] == 'unsupported' for e in trims)
    report = {'file': NAME, 'sha256': item['sha256'], 'split':'regression',
              'source_profile': {'save_version': model.metadata.save_version, 'embedded_version':22601},
              'scope':'source endpoint and Rust/OCCT consistency checks; vendor/current-state oracle absent',
              'surface_samples_each':9, 'surface_max_deviation_mm':max_surface_deviation,
              'source_tolerance_mm':converter.tolerance, 'trim_counts':counts,
              'curves':curves, 'surfaces':surfaces, 'trims':trims,
              'tolerant_endpoints':converter.tolerant_endpoints}
    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({'curves':len(curves),'surfaces':len(surfaces),'trims':counts}))


if __name__ == '__main__':
    main()
