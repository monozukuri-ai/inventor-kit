"""Source checks for tolerant boundaries and bounded ASM spline charts.

Requires acis-core and cq-acis 0.3.3 or the matching later implementation. These
component checks never claim vendor equivalence or a complete converted part.
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
    data = (ROOT / 'fixtures/public' / NAME).read_bytes()
    assert len(data) == item['bytes'] and hashlib.sha256(data).hexdigest() == item['sha256']
    doc = inventor_kit.read(data, source_id=NAME)
    model = doc.model
    converter = acis.CadQueryConverter(model)
    native = model.to_native()
    placement = converter._body_placement(model.bodies()[0])
    carrier = doc.summary['carrier']

    def provenance(entity):
        raw = entity if isinstance(entity, acis.RawEntity) else entity.raw
        span = raw.source
        offset = carrier['kernel_offset']
        assert raw.raw_data == doc.kernel_bytes[span.start_offset-offset:span.end_offset-offset]
        return dict(entity=raw.index, source=asdict(span), raw_sha256=hashlib.sha256(raw.raw_data).hexdigest())

    surfaces, boundaries, faces, pcurves = [], [], [], []
    seen_pcurves = set()
    for entity in model.entities:
        if isinstance(entity, acis.RawEntity) and entity.type_name == 'tcoedge-coedge':
            entry = provenance(entity)
            try:
                coedge = converter._require(entity.index, acis.CoedgeEntity, context='tolerant trim regression')
                edge = converter._edge(coedge, placement)
                assert edge.isValid()
                entry.update(status='converted', edge=coedge.edge.index, length_mm=edge.Length())
            except acis.CadQueryConversionError as error:
                entry.update(status='rejected', code=error.code, reason=str(error))
            boundaries.append(entry)
        surface = converter._resolve_geometry(acis.EntityRef(entity.index)) if (
            isinstance(entity, acis.RawEntity) and entity.type_name == 'spline-surface') else entity
        if isinstance(surface, acis.BSplineSurfaceEntity) and any(
            r and (r.lower is not None or r.upper is not None) for r in (surface.u_range, surface.v_range)):
            surface.validate()
            occt = converter._bspline_geometry(surface, placement)
            bounds = [(r.lower if r and r.lower is not None else knots[0],
                       r.upper if r and r.upper is not None else knots[-1])
                      for r, knots in ((surface.u_range, surface.u_knots), (surface.v_range, surface.v_knots))]
            deviation = 0.
            for u in (bounds[0][0], sum(bounds[0])/2, bounds[0][1]):
                for v in (bounds[1][0], sum(bounds[1])/2, bounds[1][1]):
                    expected = placement.point_vector(surface.evaluate(u, v))
                    actual = occt.Value(u, v)
                    deviation = max(deviation, (expected-acis.Vec3(actual.X(), actual.Y(), actual.Z())).magnitude)
            assert deviation <= converter.tolerance
            surfaces.append(dict(provenance(surface), bounds=bounds,
                                 same_poles_evaluator_max_deviation_mm=deviation))
        if not isinstance(entity, acis.FaceEntity):
            continue
        entry = provenance(entity)
        try:
            shape = converter._face(entity, placement)
            assert shape.isValid()
            entry.update(status='converted', area_mm2=shape.Area())
        except acis.CadQueryConversionError as error:
            entry.update(status='rejected', code=error.code, reason=str(error))
        faces.append(entry)
        # Read source pcurves independently of whether the complete face builds.
        for loop in converter._linked_entities(entity.loop, acis.LoopEntity, 'next_loop', context='pcurve regression'):
            try:
                coedges = converter._coedges(loop)
            except acis.CadQueryConversionError:
                continue
            for coedge in coedges:
                key = coedge.pcurve.index, entity.surface.index
                if coedge.pcurve.is_null or key in seen_pcurves:
                    continue
                seen_pcurves.add(key)
                try:
                    view = native.linear_surface_pcurve(coedge.pcurve, entity.surface)
                except acis.AcisModelError:
                    continue
                if view is not None:
                    assert view.raw == model.resolve(coedge.pcurve)
                    pcurves.append(dict(provenance(view.raw), surface=entity.surface.index,
                        support=asdict(view.support), interval=view.parameter_interval,
                        uv_endpoints=view.uv_endpoints, saved_fit_tolerance=view.fit_tolerance))

    boundary_counts = dict(Counter(e.get('code', e['status']) for e in boundaries))
    assert boundary_counts == {'converted': 195, 'geometry.tolerant_topology_unsupported': 10,
                              'geometry.tolerant_endpoint_mismatch': 1}, boundary_counts
    assert {e['entity'] for e in surfaces} == {1108, 1412, 1626, 1811, 2312, 2713, 2819, 4605}
    assert all(e['max_endpoint_deviation_mm'] <= e['tolerance_mm'] for e in converter.tolerant_boundaries)
    try:
        doc.to_cadquery()
    except acis.CadQueryConversionError as error:
        whole_part = dict(status='rejected', code=error.code, reason=str(error))
    else:
        raise AssertionError('Unqualified complete part unexpectedly accepted')
    return dict(file=NAME, sha256=item['sha256'], split='regression',
        scope='component geometry and source consistency; vendor and current Model State unverified',
        source_tolerance_mm=converter.tolerance, boundaries=boundaries, boundary_counts=boundary_counts,
        finite_surfaces=surfaces, saved_pcurve_views=pcurves, faces=faces,
        face_counts=dict(Counter(e.get('code', e['status']) for e in faces)),
        checked_boundaries=converter.tolerant_boundaries, checked_saved_pcurves=converter.saved_pcurves,
        bounded_faces=converter.bounded_surface_faces, whole_part=whole_part)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT/'internal/reports/latest/tolerant-trims.json')
    args = parser.parse_args()
    report = validate()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps(dict(boundaries=report['boundary_counts'], finite_surfaces=len(report['finite_surfaces']),
        saved_pcurve_views=len(report['saved_pcurve_views']), faces=report['face_counts'], whole_part=report['whole_part']['code'])))


if __name__ == '__main__':
    main()
