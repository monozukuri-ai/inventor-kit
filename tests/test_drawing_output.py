"""Real drawing outputs, source identity, isolation, and bounded SVG publication."""
from dataclasses import replace
import copy
import hashlib
import json
import math
import re
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import xml.etree.ElementTree as ET

import jsonschema
import inventor_kit as ik
from inventor_kit.drawing_output import MAX_SVG_BYTES, render_svg
from inventor_kit.cli import run_job
from drawing_fixture_helpers import with_segment_major

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT/'fixtures/public/SampleBg.idw'
SCHEMA = json.loads((ROOT/'schemas/drawing-report-v1.schema.json').read_text())
NS = {'s':'http://www.w3.org/2000/svg'}


class DrawingOutput(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.doc = ik.read_drawing_file(SOURCE)

    def cli(self, *args, **kwargs):
        return subprocess.run([sys.executable,'-m','inventor_kit',*map(str,args)],
                              capture_output=True,text=True,timeout=60,**kwargs)

    def test_report_has_raw_text_provenance_and_explicit_unknown_units(self):
        report=self.doc.report()
        jsonschema.validate(report,SCHEMA)
        self.assertEqual(report['source_sha256'],hashlib.sha256(SOURCE.read_bytes()).hexdigest())
        self.assertEqual(report['units'],'source_units_unverified')
        self.assertIsNone(report['millimeters_per_unit'])
        sheet=report['sheets'][0]
        self.assertEqual(sheet['item_count'],157)
        self.assertEqual(len(sheet['text_runs']),53)
        self.assertTrue(sheet['text_runs'][0]['source']['stream'])
        self.assertNotIn('items',sheet)
        details=self.doc.report(details=True)
        self.assertEqual(len(details['sheets'][0]['items']),157)
        self.assertEqual(len(self.doc.report(list_only=True)['sheets']),1)
        self.assertNotIn('text_runs',self.doc.report(list_only=True)['sheets'][0])
        with self.assertRaises(ik.DrawingDisplayError): self.doc.report(sheet_id='foreign')

    def test_svg_embeds_images_preserves_identity_and_refuses_partial_or_foreign_selection(self):
        with self.assertRaises(ik.DrawingDisplayError): self.doc.to_svg()
        with self.assertRaises(ik.DrawingDisplayError): self.doc.to_svg(sheet_id='foreign',allow_partial=True)
        with self.assertRaisesRegex(ValueError,'byte limit'): self.doc.to_svg(allow_partial=True,max_bytes=100)
        for value in (True,0,MAX_SVG_BYTES+1):
            with self.assertRaises(ValueError): self.doc.to_svg(allow_partial=True,max_bytes=value)
        data=self.doc.to_svg(allow_partial=True)
        svg=ET.fromstring(data)
        self.assertEqual(svg.attrib['data-source-sha256'],self.doc.source_sha256)
        self.assertEqual(svg.attrib['viewBox'],'0 0 42 29.7')
        self.assertEqual(len(svg.findall('.//*[@data-item-id]')),157)
        self.assertEqual(len(svg.findall('s:defs/s:image',NS)),2)
        for image in svg.findall('s:defs/s:image',NS):
            self.assertTrue(image.attrib['href'].startswith('data:image/'))
        self.assertEqual(len(svg.findall('s:path',NS)),11)
        self.assertNotIn('drawing-selected',data)
        self.assertFalse(svg.findall('.//s:script',NS))

    def test_export_pair_is_new_and_json_identifies_exact_svg(self):
        with tempfile.TemporaryDirectory() as tmp:
            path=Path(tmp)/'sheet.svg'
            report=self.doc.export_svg(path,allow_partial=True)
            jsonschema.validate(report,SCHEMA)
            self.assertEqual(report['export']['sha256'],hashlib.sha256(path.read_bytes()).hexdigest())
            self.assertEqual(report,json.loads(path.with_suffix('.svg.json').read_text()))
            with self.assertRaises(FileExistsError): self.doc.export_svg(path,allow_partial=True)
            original=path.read_bytes()
            self.assertEqual(original,self.doc.to_svg(allow_partial=True).encode())
            path.unlink()
            with self.assertRaises(FileExistsError): self.doc.export_svg(path,allow_partial=True)
            self.assertFalse(path.exists())
        multi=replace(self.doc,sheets=(self.doc.sheets[0],replace(self.doc.sheets[0],id='other',index=1)))
        with self.assertRaises(ik.DrawingDisplayError): multi.to_svg(allow_partial=True)
        self.assertTrue(multi.to_svg(sheet_id=self.doc.sheets[0].id,allow_partial=True))

    def test_report_cli_batches_continue_and_svg_publication_uses_final_path(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp=Path(tmp); bad=tmp/'bad.idw'; bad.write_bytes(b'bad')
            result=self.cli(bad,SOURCE,'--drawing-report','--jsonl')
            self.assertEqual(result.returncode,1,result.stderr)
            rows=[json.loads(line) for line in result.stdout.splitlines()]
            self.assertEqual([r['status'] for r in rows],['error','partial'])
            for row in rows: jsonschema.validate(row,SCHEMA)
            svg=tmp/'saved.svg'
            refused=self.cli(SOURCE,'--svg',svg)
            self.assertEqual(refused.returncode,1)
            self.assertFalse(svg.exists())
            result=self.cli(SOURCE,'--svg',svg,'--allow-partial')
            self.assertEqual(result.returncode,2,result.stderr)
            report=json.loads(result.stdout);jsonschema.validate(report,SCHEMA)
            sidecar=json.loads(svg.with_suffix('.svg.json').read_text())
            self.assertEqual(report['export']['path'],str(svg))
            self.assertEqual(sidecar['export']['path'],str(svg))
            self.assertEqual(report['export']['sha256'],hashlib.sha256(svg.read_bytes()).hexdigest())
            self.assertEqual(self.cli(SOURCE,'--svg',svg,'--allow-partial').returncode,1)
            invalid=self.cli(SOURCE,'--drawing-report','--step',tmp/'never.step')
            self.assertEqual(invalid.returncode,1)
            self.assertFalse((tmp/'never.step').exists())
        with tempfile.TemporaryDirectory() as tmp:
            source=Path(tmp)/'synthetic-major25.idw'
            source.write_bytes(with_segment_major(SOURCE.read_bytes(),25))
            unsupported=self.cli(source,'--drawing-report')
            self.assertEqual(unsupported.returncode,3)
            jsonschema.validate(json.loads(unsupported.stdout),SCHEMA)
        foreign=self.cli(ROOT/'fixtures/public/SamplePart.ipt','--list-sheets')
        self.assertEqual(foreign.returncode,3)

    def test_idw_cli_never_imports_geometry_and_crashes_do_not_publish_svg(self):
        import os
        with tempfile.TemporaryDirectory() as tmp:
            tmp=Path(tmp)
            (tmp/'sitecustomize.py').write_text('''import sys
class NoGeometry:
    def find_spec(self, fullname, path=None, target=None):
        if fullname.split('.')[0] in {'cq_acis','cadquery','OCP','ocp_tessellate'}:
            raise ImportError('Unexpected 3D dependency')
sys.meta_path.insert(0,NoGeometry())
''')
            environment=dict(os.environ,PYTHONPATH=str(tmp)+os.pathsep+str(ROOT/'python'))
            result=self.cli(SOURCE,'--list-sheets',env=environment)
            self.assertEqual(result.returncode,2,result.stderr)
            target=tmp/'crashed.svg'
            def crash(command,**kwargs):
                request=json.loads(kwargs['input']);Path(request['svg']).write_text('partial')
                return subprocess.CompletedProcess(command,-11)
            with patch('inventor_kit.cli.subprocess.run',side_effect=crash):
                report=run_job(dict(path=str(SOURCE),operation='drawing',svg=str(target)),1)
            self.assertEqual(report['diagnostics'][0]['code'],'execution.worker_failed')
            self.assertFalse(target.exists())
            jsonschema.validate(report,SCHEMA)
            with patch('inventor_kit.cli.subprocess.run',side_effect=subprocess.TimeoutExpired('worker',1)):
                report=run_job(dict(path=str(SOURCE),operation='drawing',svg=str(target)),1)
            self.assertEqual(report['diagnostics'][0]['code'],'execution.timeout')
            self.assertFalse(target.exists())
            jsonschema.validate(report,SCHEMA)

    def test_renderer_preserves_literal_cjk_symbol_dash_and_affine_arc(self):
        style=dict(width=.02,rgba=[0,0,0,1],dash=[.5,.1],unresolved=[])
        font=dict(family='Missing "font", serif',height_candidate=1,weight_candidate=400,flags=0,width_factor=1)
        text=dict(kind='text',text='日本語 <script>\x00\n±90°',position=[1,2,0],direction=[1,0,0],up=[0,1,0],raw_flags=9,font=font)
        curve=dict(kind='curve',center=[5,5,0],u=[2,1,0],v=[1,3,0],start=0,end=2*math.pi)
        symbol=dict(text, text='n',font=dict(font,family='AIGDT'))
        depth=dict(symbol, text='x')
        sheet=dict(id='test',name='Test',status='experimental_partial',size_in_source_units=[10,10],
            items=[dict(id=str(i),geometry=g,style=style) for i,g in enumerate((text,curve,symbol,depth))])
        xml=ET.fromstring(render_svg(sheet,'a'*64,{}))
        line=xml.find('s:path',NS)
        self.assertEqual(line.attrib['stroke-dasharray'],'0.5 0.1')
        self.assertEqual(line.attrib['d'].count('A '),4)
        self.assertTrue(line.attrib['d'].startswith('M 7 4 '))
        import numpy as np
        radii=np.linalg.svd(np.array([[2.,1.],[-1.,-3.]]),compute_uv=False)
        encoded=re.search(r'A (\S+) (\S+) ',line.attrib['d'])
        for actual,expected in zip(map(float,encoded.groups()),radii):
            self.assertAlmostEqual(actual,expected,places=12)
        texts=xml.findall('s:text',NS)
        self.assertEqual(texts[0].attrib['data-font-fallback'],'system-cjk')
        self.assertIn('Noto Sans CJK JP',texts[0].attrib['font-family'])
        self.assertEqual(texts[0].find('s:tspan',NS).text,'日本語 <script>\ufffd')
        self.assertEqual(texts[1].find('s:tspan',NS).text,'⌀')
        self.assertEqual(texts[1].attrib['data-raw-text'],'n')
        self.assertEqual(texts[2].find('s:tspan',NS).text,'↧')
        self.assertEqual(texts[2].attrib['data-symbol-fallback'],'AIGDT:x->U+21A7')
        self.assertEqual(texts[2].attrib['data-raw-text'],'x')

    def test_straight_dash_fitting_preserves_source_and_rotated_or_reversed_endpoints(self):
        style = dict(width=.02, rgba=[0,0,0,1], dash=[2.,2.],
                     unresolved=['dash_phase_and_fit_unverified'])
        sheet = dict(id='test', status='experimental_partial', size_in_source_units=[30,30],
                     segment_majors={'segment':31})
        for start, end in [([1,1,0],[13,1,0]), ([13,1,0],[1,1,0]),
                           ([1,1,0],[1,13,0]), ([1,1,0],[8.2,10.6,0])]:
            item = dict(id='line', segment_id='segment',
                        geometry=dict(kind='polyline',points=[start,end]),style=style)
            original = copy.deepcopy(item)
            node = ET.fromstring(render_svg(dict(sheet,items=[item]),'a'*64,{})).find('s:polyline',NS)
            self.assertEqual(node.get('stroke-dasharray'),'2 2')
            self.assertEqual(node.get('stroke-dashoffset'),'1')
            self.assertEqual(node.get('data-dash-rendering'),'whole-period-half-dash-ends')
            self.assertEqual(item, original)
            points = [list(map(float,p.split(','))) for p in node.get('points').split()]
            self.assertEqual(points, [[p[0],30-p[1]] for p in (start,end)])

    def test_dash_fitting_is_limited_to_observed_profiles_and_straight_geometry(self):
        line = dict(kind='polyline',points=[[1,1,0],[2.2,1,0]])
        sheet = dict(id='test',status='experimental_partial',size_in_source_units=[30,30])
        def node(g=line, dash=(4.,1.), major=26):
            item = dict(id='line',segment_id='s',geometry=g,style=dict(dash=dash))
            svg = ET.fromstring(render_svg(dict(sheet,items=[item],segment_majors={'s':major}), 'a'*64, {}))
            return svg.find('.//*[@data-item-id]')
        for major in (26,28,29):
            n = node(major=major)
            self.assertEqual(n.get('stroke-dasharray'),'0.4 0.4')
            self.assertEqual(n.get('stroke-dashoffset'),'0')
            self.assertEqual(n.get('data-dash-rendering'),'short-line-thirds')
        self.assertEqual(node(dash=(5.,1.,1.,1.),major=24).get('data-dash-rendering'),'short-line-thirds')
        unknown = [dict(major=23),dict(major=None),dict(major=31),dict(dash=(3.,1.)),
            dict(g=dict(line,points=[[1,1,0],[8.5,1,0]])),  # Half-period tie.
            dict(g=dict(line,points=[[1,1,0],[1,1,0]])),
            dict(g=dict(line,points=[[1,1,0],[1.6,1,0],[2.2,1,0]])),
            dict(g=dict(kind='curve',center=[1,1,0],u=[1,0,0],v=[0,1,0],start=0,end=math.pi)),
            dict(g=dict(line,points=[[1,1,0],[10,1,0]]),dash=(5.,1.,1.,1.),major=24),
            dict(dash=(4e-300,1e-300)),dict(dash=(4.,1e-300))]
        for kwargs in unknown:
            with self.subTest(kwargs=kwargs):
                n = node(**kwargs)
                self.assertEqual(n.get('data-dash-rendering'),'nominal-unverified')
                self.assertNotIn('stroke-dashoffset',n.attrib)
        for dash in ((0.,1.),(1.,),(-1.,2.),(float('nan'),1.),(float('inf'),1.),(True,1.)):
            with self.subTest(dash=dash), self.assertRaises(ValueError): node(dash=dash)

    def test_real_major_profiles_supply_svg_fit_metadata_without_mutating_nominal_styles(self):
        for name, count in [('iacs/Template_IACS',2),('versions/Toys-R-Us-Rex',14),('versions/RespiraWorks',21)]:
            doc = ik.read_drawing_file(ROOT/'fixtures/public/drawings'/(name+'.idw'))
            before = doc.report(details=True)
            data = doc.to_svg(allow_partial=True)
            svg = ET.fromstring(data)
            fitted = [n for n in svg if n.get('data-dash-rendering') in
                      ('short-line-thirds','whole-period-half-dash-ends')]
            self.assertEqual(len(fitted),count)
            self.assertEqual(doc.report(details=True),before)
            from inventor_kit.drawing_output import svg_report
            report = svg_report(doc,doc.sheets[0],data.encode())
            self.assertEqual(report['export']['dash_rendering']['short_lines']+
                             report['export']['dash_rendering']['whole_period_lines'],count)
            jsonschema.validate(report,SCHEMA)

    def test_cap_height_and_symbol_profiles_remain_bounded(self):
        from inventor_kit.drawing_output import text_presentation
        font=dict(family='ISOCP', height_candidate=.25, weight_candidate=400, flags=0)
        g=dict(kind='text', text='H', raw_flags=9, font=font)
        for family, weight in [('ISOCP',400),('ISOCP_IV25',400),('ISOCP_IV25',700),
                               ('Vafle VUT',400),('Vafle Light VUT',400)]:
            self.assertTrue(text_presentation(dict(g,font=dict(font,family=family,weight_candidate=weight)))['capital'])
        for changed in [dict(family='Unmeasured'),dict(flags=2),dict(weight_candidate=700),dict(height_candidate=0)]:
            self.assertFalse(text_presentation(dict(g,font=dict(font,**changed)))['capital'])
        for text in ['x', 'n']:
            symbol=dict(g,text=text,font=dict(font,family='AIGDT'))
            self.assertTrue(text_presentation(symbol)['symbol'])
            for changed in [dict(text=text+'1'),dict(raw_flags=10),dict(font=font),
                            dict(font=dict(symbol['font'],flags=1))]:
                value=dict(symbol,**changed)
                self.assertFalse(text_presentation(value)['symbol'])
                self.assertEqual(text_presentation(value)['text'],value['text'])

    def test_filled_arc_closes_by_chord_and_has_no_outline(self):
        g=dict(kind='curve', center=[2,3,0], u=[1,0,0], v=[0,1,0], start=0, end=math.pi/2, filled=True)
        sheet=dict(id='filled',name='Origin',status='experimental_partial',size_in_source_units=[10,10],
                   items=[dict(id='segment',geometry=g,style=dict(rgba=[0,0,0,1],width=.02,dash=[],unresolved=[]))])
        node=ET.fromstring(render_svg(sheet,'a'*64,{})).find('s:path',NS)
        self.assertEqual(node.attrib['fill'],'rgb(0,0,0)')
        self.assertEqual(node.attrib['stroke'],'none')
        self.assertTrue(node.attrib['d'].startswith('M 3 7 A '))
        self.assertTrue(node.attrib['d'].endswith(' 2 6 Z'))
        self.assertNotIn('L ',node.attrib['d'])  # no invented center/radial segment

    def test_saved_triangles_fill_indexed_vertices_and_reject_invalid_geometry(self):
        geometry = dict(kind='triangles', vertices=[[1., 2., 0.], [4., 2., 0.], [1., 5., 0.]], indices=[2, 0, 1])
        def render(g):
            return render_svg(dict(id='triangles', name='Arrows', status='experimental_partial',
                size_in_source_units=[10, 10], items=[dict(id='arrow', geometry=g,
                style=dict(rgba=[1, 0, 0, 1], width=.02, dash=[.5, .1], unresolved=[]))]), 'a'*64, {})
        node = ET.fromstring(render(geometry)).find('s:path', NS)
        self.assertEqual(node.attrib['d'], 'M1,5 L1,8 L4,8 Z')
        self.assertEqual(node.attrib['stroke'], 'none')
        self.assertNotEqual(node.attrib['fill'], 'none')
        self.assertNotIn('stroke-dasharray', node.attrib)
        for change in [dict(indices=[0, 1, 3]), dict(indices=[0, True, 2]), dict(indices=[0, 1]),
                       dict(indices=[0, 0, 2]), dict(vertices=[[1., 2., 0.], [4., 2., 1.], [1., 5., 0.]]),
                       dict(vertices=[[float('nan'), 2., 0.], [4., 2., 0.], [1., 5., 0.]])]:
            with self.subTest(change=change), self.assertRaises(ValueError):
                render(dict(geometry, **change))

    def test_edge_on_ellipse_keeps_turning_points_without_unstable_svg_arcs(self):
        # A complete edge-on projection must travel to both extrema and back;
        # replacing it with one endpoint-to-endpoint line would erase it.
        g=dict(kind='curve',center=[5,5,0],u=[2,1,0],v=[1e-15,1e-15,3],start=0,end=2*math.pi)
        sheet=dict(id='edge-on',name='Projection',status='experimental_partial',size_in_source_units=[10,10],
            items=[dict(id='curve',geometry=g,style={})])
        node=ET.fromstring(render_svg(sheet,'a'*64,{})).find('s:path',NS)
        self.assertNotIn('A ',node.attrib['d'])
        self.assertIn('L 3 6',node.attrib['d'])
        self.assertLess(float(node.attrib['data-projection-error-bound']),2e-9)
        self.assertEqual(node.attrib['data-curve-rendering'],'near-degenerate-line-projection')
        g['v']=[0,0,3]  # exact zero minor radius also preserves the projection
        self.assertIn('L 3 6',render_svg(sheet,'a'*64,{}))
