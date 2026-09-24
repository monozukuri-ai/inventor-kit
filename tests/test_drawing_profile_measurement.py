"""Independent-coordinate and malformed-trace checks for the native measurement tool."""
import math
from pathlib import Path
import sys
import unittest
import xml.etree.ElementTree as ET
from types import SimpleNamespace

sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'scripts'))
from measure_drawing_profiles import trace_paths, polygon_mm, triangle_distance, point_segment_distance, filled_conics, hidden_line_patterns, PT_TO_MM


class ProfileMeasurement(unittest.TestCase):
    def test_hidden_line_measurement_checks_the_actual_svg_phase_independently(self):
        sheet = SimpleNamespace(items=[SimpleNamespace(id='line',geometry=dict(kind='polyline',
            points=[[0,0,0],[1.5,0,0]]),style=dict(dash=[.4,.1]))])
        trace = ''.join(f'<stroke_path transform="1 0 0 -1 0 100"><moveto x="{a/PT_TO_MM}" y="0"/>'
                        f'<lineto x="{b/PT_TO_MM}" y="0"/></stroke_path>'
                        for a,b in [(0,2),(3,7),(8,12),(13,15)])
        svg = ET.fromstring('<svg><polyline data-item-id="line" points="0,1 1.5,1" '
                            'stroke-dasharray=".4 .1" stroke-dashoffset=".2"/></svg>')
        good = hidden_line_patterns(sheet,trace,[0,0,100,100],svg)
        self.assertTrue(good['svg_phase_and_fitting_matches_pdf'])
        self.assertLess(good['rows'][0]['svg_max_endpoint_error_mm'],1e-12)
        svg[0].set('stroke-dashoffset','0')
        bad = hidden_line_patterns(sheet,trace,[0,0,100,100],svg)
        self.assertTrue(bad['rows'][0]['matched'])
        self.assertFalse(bad['svg_phase_and_fitting_matches_pdf'])

    def test_filled_conic_comparison_does_not_fit_translation_or_radius(self):
        for closed,count in [(False,4),(True,8),(True,12)]:
            g=dict(kind='curve',filled=True,center=[2,3,0],u=[1,0,0],v=[0,1,0],start=0,
                   end=2*math.pi if closed else math.pi/2)
            node=ET.Element('fill_path',transform='1 0 0 -1 0 100')
            for k in range(count+1):
                t=g['end']*k/count
                # Make the duplicate closure exact, as in MuPDF's trace.
                if closed and k==count: t=0
                ET.SubElement(node,'moveto' if k==0 else 'lineto',
                    x=str(10*(2+math.cos(t))/PT_TO_MM),y=str(10*(3+math.sin(t))/PT_TO_MM))
            ET.SubElement(node,'closepath')
            sheet=SimpleNamespace(items=[SimpleNamespace(id='conic',geometry=g)])
            result=filled_conics(sheet,[node],[0,0,100,100])
            self.assertEqual(result['matched_count'],1)
            self.assertLess(result['max_vertex_error_mm'],1e-12)
            for changed in [dict(center=[2.1,3,0]),dict(u=[1.1,0,0])]:
                shifted=SimpleNamespace(items=[SimpleNamespace(id='conic',geometry=dict(g,**changed))])
                self.assertEqual(filled_conics(shifted,[node],[0,0,100,100])['matched_count'],0)

    def test_pdf_transform_and_bottom_left_origin_are_applied_once(self):
        node=ET.fromstring('<fill_path transform="2 0 0 -2 10 100"><moveto x="0" y="0"/>'
            '<lineto x="1" y="0"/><lineto x="0" y="1"/><closepath/></fill_path>')
        points=polygon_mm(node,[0,0,120,100])
        expected=[[10*25.4/72,0],[12*25.4/72,0],[10*25.4/72,2*25.4/72]]
        self.assertLess(triangle_distance(points,list(reversed(expected))),1e-12)
        shifted=[[x+1,y] for x,y in expected]
        self.assertGreaterEqual(triangle_distance(points,shifted),.99)
        self.assertEqual(point_segment_distance([2,1],[0,0],[1,0]),math.sqrt(2))

    def test_complete_path_fragments_survive_unbalanced_layer_tags_only(self):
        path='<fill_path transform="1 0 0 1 0 0"><moveto x="0" y="0"/></fill_path>'
        trace='<document><page mediabox="0 0 10 20"><layer>'+path+'</page></document>'
        box,nodes,status=trace_paths(trace)
        self.assertEqual(box,[0,0,10,20]); self.assertEqual(len(nodes),1)
        self.assertTrue(status.startswith('unbalanced_container'))
        self.assertIsNone(polygon_mm(nodes[0],box))
        for broken in [trace.replace('</fill_path>',''),trace+trace,trace.replace('0 0 10 20','0 0 nan 20')]:
            with self.assertRaises(ValueError): trace_paths(broken)


if __name__=='__main__': unittest.main()
