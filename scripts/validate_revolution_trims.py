"""FTC10/CTC02 cylindrical trims: retained source edges, STEP and mesh coverage.

Requires the cq-acis cylinder_slit_faces and elliptic_sections diagnostics.
This regression gate checks individual faces; both complete parts still have
unsupported intersection curves/surfaces. No holdout is used for qualification.
"""
import argparse
from collections import Counter
from dataclasses import asdict
import hashlib
import importlib.metadata
import json
import math
from pathlib import Path

import cadquery as cq
import cq_acis as acis
import inventor_kit
from OCP.BRep import BRep_Tool
from corpus_manifest import load_manifest
from inventor_kit.viewer.scene import Options, build_scene
from inventor_kit.viewer.tessellation import MeshBuilder
from oracle_contract import shape_metrics

ROOT = Path(__file__).resolve().parents[1]
TARGETS = {
    'INV_nist_ftc_10_asme1_2021.ipt': dict(faces=223, valid=198, repaired={358,575,877,1387},
        failures={'geometry.curve_unsupported':23,'geometry.surface_unsupported':2}),
    'INV_nist_ctc_02_asme1_2021.ipt': dict(faces=443, valid=411, repaired={587,660,1004,1589,2540,4120},
        failures={'geometry.curve_unsupported':16,'geometry.surface_unsupported':16}),
}


def validate(item, expected, directory):
    name = item['file']
    data = (ROOT/'fixtures/public'/name).read_bytes()
    assert len(data)==item['bytes'] and hashlib.sha256(data).hexdigest()==item['sha256']
    doc = inventor_kit.read(data,source_id=name)
    c = acis.CadQueryConverter(doc.model)
    assert hasattr(c,'cylinder_slit_faces'), 'cq-acis with qualified cylinder slit support is required'
    model = c.model
    assert model.to_native().to_model()==model
    placements = {b.index:c._body_placement(b) for b in model.bodies()}
    directory.mkdir(parents=True,exist_ok=True)
    mesh_builder = MeshBuilder(directory,Options())
    valid, failures, repaired = [], [], []
    for face in (e for e in model.entities if isinstance(e,acis.FaceEntity)):
        shell = model.resolve(face.shell)
        body_index = model.resolve(shell.lump).body.index
        placement = placements[body_index]
        try:
            shape = c._face(face,placement)
        except acis.CadQueryConversionError as error:
            assert face.index not in expected['repaired']
            failures.append(dict(face=face.index,body=body_index,code=error.code,reason=str(error)))
            continue
        assert shape.isValid() and shape.Area()>0
        valid.append(dict(face=face.index,body=body_index,area_mm2=shape.Area()))
        if face.index not in expected['repaired']:
            continue
        loops = c._linked_entities(face.loop,acis.LoopEntity,'next_loop',context='retained source')
        uses = [co for loop in loops for co in c._coedges(loop)]
        edges = {co.edge.index:c._require(co.edge,acis.EdgeEntity,context='retained source') for co in uses}
        vertex_refs = {v.index for e in edges.values() for v in (e.start_vertex,e.end_vertex)}
        points = [placement.point(c._vertex_location(acis.EntityRef(v),context='retained source')) for v in vertex_refs]
        sources = {face.index:face, face.surface.index:model.resolve(face.surface)}
        sources.update({l.index:l for l in loops});sources.update({co.index:co for co in uses});sources.update(edges)
        for edge in edges.values():
            sources[edge.curve.index] = model.resolve(edge.curve)
            for ref in (edge.start_vertex,edge.end_vertex):
                vertex = model.resolve(ref);sources[ref.index] = vertex;sources[vertex.point.index] = model.resolve(vertex.point)
        provenance = []
        for index,entity in sorted(sources.items()):
            raw = entity.raw
            span = raw.source
            offset = doc.summary['carrier']['kernel_offset']
            assert raw.raw_data==doc.kernel_bytes[span.start_offset-offset:span.end_offset-offset]
            provenance.append(dict(entity=index,type=raw.type_name,source=asdict(span),raw_sha256=hashlib.sha256(raw.raw_data).hexdigest()))
        step = directory/f'face-{face.index}.step'
        cq.exporters.export(shape,str(step))
        roundtrip = cq.importers.importStep(str(step)).val()
        assert roundtrip.isValid() and len(roundtrip.Faces())==1
        # Ordinary INTERNAL edges disappear in STEP; retained seam segments must
        # preserve the edge/vertex counts as well as the surface and source points.
        assert len(roundtrip.Edges())==len(shape.Edges()) and len(roundtrip.Vertices())==len(shape.Vertices())
        seam_counts = [sum(BRep_Tool.IsClosed_s(e.wrapped,s.wrapped) for e in s.Edges())
                       for s in (shape,roundtrip.Faces()[0])]
        assert seam_counts[0]==seam_counts[1]
        slit_check = next((entry for entry in c.cylinder_slit_faces if entry['face']==face.index),None)
        if slit_check is not None:
            assert seam_counts[1]==slit_check['seam_segments']
            for paired in slit_check['paired_edges']:
                edge = edges[paired['edge']]
                endpoints = [placement.point(c._vertex_location(v,context='STEP retained line'))
                             for v in (edge.start_vertex,edge.end_vertex)]
                matches = [e for e in roundtrip.Edges() if e.geomType()=='LINE' and
                           all(any(math.dist(p,v.toTuple())<=c.tolerance for v in e.Vertices()) for p in endpoints)]
                assert len(matches)==1
        before,after = shape_metrics((shape,)),shape_metrics((roundtrip,))
        assert math.isclose(before['area_mm2'],after['area_mm2'],rel_tol=1e-8,abs_tol=1e-6)
        assert all(abs(a-b)<=1e-5 for a,b in zip(before['bbox_mm'],after['bbox_mm']))
        for point in points:
            assert all(any(math.dist(point,v.toTuple())<=c.tolerance for v in s.Vertices()) for s in (shape,roundtrip))
        mesh = mesh_builder.add(shape,f'face-{face.index}')
        assert mesh['face_count']==1 and mesh['triangle_count']>0
        repaired.append(dict(face=face.index,body=body_index,metrics=before,source_edges=list(edges),source_vertices=sorted(vertex_refs),
            source_coedges=[co.index for co in uses],source_provenance=provenance,
            output_edges=len(shape.Edges()),output_vertices=len(shape.Vertices()),mesh=mesh,
            seam_edges=seam_counts[0],step_seam_edges=seam_counts[1],
            step_roundtrip='passed',step_file=str(step),step_sha256=hashlib.sha256(step.read_bytes()).hexdigest()))
    assert len(valid)==expected['valid'] and len(valid)+len(failures)==expected['faces']
    assert Counter(e['code'] for e in failures)==expected['failures']
    assert {f['face'] for f in repaired}==expected['repaired']
    seam_checks = [e for e in c.periodic_seam_faces if e['face'] in expected['repaired']]
    slit_checks = c.cylinder_slit_faces
    assert all(e['max_deviation_mm']<=e['tolerance_mm'] for e in [*seam_checks,*slit_checks])
    if 'ctc' in name:
        assert len(seam_checks)==6 and all(e['elliptic_sections'] for e in seam_checks) and not slit_checks
    else:
        assert len(seam_checks)==3 and len(slit_checks)==1
        assert slit_checks[0]['face']==1387 and {e['edge'] for e in slit_checks[0]['paired_edges']}=={884,1189,1612}
        assert slit_checks[0]['seam_segments']==7
    try:
        doc.to_cadquery()
    except acis.CadQueryConversionError as error:
        assert error.code in ('geometry.curve_unsupported','geometry.surface_unsupported')
        whole = dict(status='rejected',code=error.code,reason=str(error))
    else:
        raise AssertionError('Unsupported whole-part conversion unexpectedly succeeded')
    # A face-only diagnostic mesh must never become an automatic partial preview.
    viewer_dir = directory/'viewer';viewer_dir.mkdir(exist_ok=True)
    scene = build_scene(ROOT/'fixtures/public'/name,viewer_dir,Options())
    assert scene['stages']['conversion']=='failed' and not scene['meshes'] and not scene['complete']
    return dict(file=name,sha256=item['sha256'],split=item['split'],valid_faces=valid,failures=failures,
        repaired_faces=repaired,seam_checks=seam_checks,slit_checks=slit_checks,whole_part=whole,
        preview_scope='repaired individual source faces only; not a complete part',viewer=scene['stages'])


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,default=ROOT/'internal/reports/latest/revolution-trims.json')
    args=parser.parse_args()
    items={e['file']:e for e in load_manifest(split='regression')}
    results=[]
    for name,expected in TARGETS.items():
        entry=validate(items[name],expected,args.output.parent/'revolution-trim-artifacts'/Path(name).stem)
        results.append(entry)
        print(name,len(entry['valid_faces']),'valid;',len(entry['repaired_faces']),'repaired;',entry['whole_part']['code'],flush=True)
    native=Path(acis._native.__file__).resolve()
    report=dict(scope='Local saved face geometry; complete parts, vendor and current Model State remain unqualified',
        holdouts_used=False,packages={p:importlib.metadata.version(p) for p in ('inventor-kit','cq-acis','cadquery','cadquery-ocp')},
        module_files=dict(cq_acis=acis.__file__,inventor_kit=inventor_kit.__file__,acis_native=str(native)),
        native_sha256=hashlib.sha256(native.read_bytes()).hexdigest(),core_version=acis._native.CORE_VERSION,results=results)
    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.write_text(json.dumps(report,indent=2,allow_nan=False)+'\n')


if __name__=='__main__':
    main()
