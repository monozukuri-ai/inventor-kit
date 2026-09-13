"""Source checks for tolerant boundaries and bounded ASM spline charts.

Requires acis-core and cq-acis 0.3.4 or the matching later implementation. These
component checks never claim vendor equivalence or a complete converted part.
"""
import argparse
from collections import Counter
from dataclasses import asdict
import hashlib
import importlib.metadata
import json
import math
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

    surfaces, boundaries, faces, pcurves, spline_pcurves = [], [], [], [], []
    spline_view = getattr(native, 'spline_surface_pcurve', None)
    associated_view = getattr(native, 'supported_curve', None)
    inline_views, associated_views = [], []
    seen_pcurves = set()
    for entity in model.entities:
        if isinstance(entity, acis.RawEntity) and entity.type_name == 'tcoedge-coedge':
            entry = provenance(entity)
            if associated_view is not None:
                inline = native.tolerant_topology(entity.index).inline_curve
                if inline is not None:
                    assert inline.curve.raw == entity
                    assert (inline.value_start, inline.value_end) == (14, len(entity.values)-1)
                    inline_views.append(dict(provenance(entity), kind=inline.kind,
                        value_start=inline.value_start, value_end=inline.value_end,
                        support_type=type(inline.support).__name__, uv_degree=inline.pcurve.degree,
                        fit_tolerance=inline.curve.fit_tolerance))
            try:
                coedge = converter._require(entity.index, acis.CoedgeEntity, context='tolerant trim regression')
                edge = converter._edge(coedge, placement)
                assert edge.isValid()
                entry.update(status='converted', edge=coedge.edge.index, length_mm=edge.Length())
            except acis.CadQueryConversionError as error:
                entry.update(status='rejected', code=error.code, reason=str(error))
            boundaries.append(entry)
        if associated_view is not None and isinstance(entity, acis.RawEntity) and entity.type_name == 'intcurve-curve':
            view = associated_view(entity.index)
            if view is not None:
                assert view.curve.raw == entity
                assert view.support.raw == entity
                owner = model.resolve(view.support_definition.entity_index)
                associated_views.append(dict(provenance(entity), kind=view.kind,
                    support_definition=asdict(view.support_definition), support_owner=provenance(owner),
                    value_start=view.value_start, value_end=view.value_end,
                    fit_tolerance=view.curve.fit_tolerance,
                    knots=view.curve.knots, multiplicities=view.curve.multiplicities,
                    poles=[asdict(p) for p in view.curve.poles], uv_poles=view.pcurve.poles,
                    support_range=asdict(view.support_range), saved_lists=view.saved_lists))
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
                if spline_view is not None:
                    try:
                        view = spline_view(coedge.pcurve, entity.surface)
                    except acis.AcisModelError:
                        continue
                    if view is not None:
                        assert view.raw == model.resolve(coedge.pcurve)
                        spline_pcurves.append(dict(provenance(view.raw), surface=entity.surface.index,
                            support=asdict(view.support), interval=view.parameter_interval,
                            degree=view.degree, knots=view.knots, multiplicities=view.multiplicities,
                            poles=view.poles, reversed=view.reversed, support_reversed=view.support_reversed,
                            saved_fit_tolerance=view.fit_tolerance))

    boundary_counts = dict(Counter(e.get('code', e['status']) for e in boundaries))
    expected_boundaries = {'converted':206} if associated_view is not None else {
        'converted':195, 'geometry.tolerant_topology_unsupported':10, 'geometry.tolerant_endpoint_mismatch':1}
    assert boundary_counts == expected_boundaries, boundary_counts
    assert {e['entity'] for e in surfaces} == {1108, 1412, 1626, 1811, 2312, 2713, 2819, 4605}
    assert all(e['max_endpoint_deviation_mm'] <= e['tolerance_mm'] for e in converter.tolerant_boundaries)
    source_bounds = getattr(converter, 'source_edge_tolerances', [])
    for checked in source_bounds:
        edge = native.tolerant_topology(checked['edge'])
        assert isinstance(edge, acis.TolerantEdge)
        assert checked['saved_scalar_source_units'] == edge.saved_scalar
        expected = edge.saved_scalar * placement.vector(acis.Vec3(1., 0., 0.)).magnitude
        assert math.isclose(checked['saved_deviation_mm'], expected, rel_tol=1e-14)
        assert checked['model_resolution_mm'] == converter.tolerance
        assert checked['limit_mm'] == expected + converter.tolerance
        assert 0 <= checked['max_deviation_mm'] <= checked['limit_mm']
        assert checked['max_deviation_mm'] > converter.tolerance or 'inline_coedge' in checked
        provenance(model.resolve(acis.EntityRef(checked['edge'])))
    if spline_view is not None:
        assert len(pcurves) == 153
        assert Counter(e['degree'] for e in spline_pcurves) == {1: 219, 3: 5}
        face_counts = Counter(e.get('code', e['status']) for e in faces)
        expected_faces = {'converted':254, 'geometry.pcurve_mismatch':2, 'geometry.spline_endpoint_mismatch':2} if associated_view is not None else {
            'converted':245, 'geometry.tolerant_topology_unsupported':10, 'geometry.pcurve_mismatch':2, 'geometry.curve_unsupported':1}
        assert face_counts == expected_faces, face_counts
        assert {e['face'] for e in converter.bounded_surface_faces} == {731, 995, 1343, 1831, 2206}
        assert any(e['degree'] == 3 for e in spline_pcurves)
        assert any(e['reversed'] for e in spline_pcurves)
        assert any(e['support_reversed'] for e in spline_pcurves)
        assert source_bounds
    if associated_view is not None:
        assert {e['entity'] for e in inline_views} == {1069,1075,1615,1868,1871,2499,3184,3463,3616,3770}
        assert [e['entity'] for e in associated_views] == [4022]
        checks = converter.supported_curve_checks
        assert {e['entity'] for e in checks if e['kind']=='par_int_cur'} == {e['entity'] for e in inline_views}
        for check in checks:
            assert check['max_deviation_mm'] <= check['tolerance_mm']
            if check['kind']=='par_int_cur':
                assert check['shared_edge_max_deviation_mm'] <= check['shared_edge_tolerance_mm']
            else:
                assert check['secondary_support_bound_mm'] <= check['tolerance_mm']
        assert {e['coedge'] for e in converter.inline_reparameterizations} == {1615,3616}
        for check in converter.inline_reparameterizations:
            assert max(check['locus_bound_mm'], check['endpoint_deviation_mm']) <= converter.tolerance
        assert {e['vertex'] for e in converter.spline_endpoint_failures} == {3618}
        assert {e['face'] for e in converter.plane_wire_orientations} == {2782}
    try:
        doc.to_cadquery()
    except acis.CadQueryConversionError as error:
        whole_part = dict(status='rejected', code=error.code, reason=str(error))
    else:
        raise AssertionError('Unqualified complete part unexpectedly accepted')
    return dict(file=NAME, sha256=item['sha256'], split='regression',
        packages={name: importlib.metadata.version(name) for name in ('inventor-kit', 'cq-acis', 'cadquery', 'cadquery-ocp')},
        module_files=dict(cq_acis=acis.__file__, inventor_kit=inventor_kit.__file__),
        scope='component geometry and source consistency; vendor and current Model State unverified',
        source_tolerance_mm=converter.tolerance, boundaries=boundaries, boundary_counts=boundary_counts,
        finite_surfaces=surfaces, saved_pcurve_views=pcurves, saved_spline_pcurve_views=spline_pcurves, faces=faces,
        face_counts=dict(Counter(e.get('code', e['status']) for e in faces)),
        checked_boundaries=converter.tolerant_boundaries, checked_saved_pcurves=converter.saved_pcurves,
        inline_curve_views=inline_views, associated_curve_views=associated_views,
        supported_curve_checks=getattr(converter, 'supported_curve_checks', []),
        inline_pcurves=getattr(converter, 'inline_pcurves', []),
        inline_reparameterizations=getattr(converter, 'inline_reparameterizations', []),
        associated_pcurves=getattr(converter, 'associated_pcurves', []),
        tolerant_line_trims=getattr(converter, 'tolerant_line_trims', []),
        tolerant_pcurve_joins=getattr(converter, 'tolerant_pcurve_joins', []),
        plane_wire_orientations=getattr(converter, 'plane_wire_orientations', []),
        spline_endpoint_failures=getattr(converter, 'spline_endpoint_failures', []),
        source_edge_tolerances=source_bounds, pcurve_checks=getattr(converter, 'pcurve_checks', None),
        bounded_faces=converter.bounded_surface_faces, whole_part=whole_part)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT/'internal/reports/latest/tolerant-trims.json')
    args = parser.parse_args()
    report = validate()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps(dict(boundaries=report['boundary_counts'], finite_surfaces=len(report['finite_surfaces']),
        saved_pcurve_views=len(report['saved_pcurve_views']), saved_spline_pcurve_views=len(report['saved_spline_pcurve_views']),
        faces=report['face_counts'], whole_part=report['whole_part']['code'])))


if __name__ == '__main__':
    main()
