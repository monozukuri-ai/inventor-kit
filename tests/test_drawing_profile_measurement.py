"""Independent-coordinate and malformed-trace checks for the native measurement tool."""
import math
from pathlib import Path
import sys
import unittest
import xml.etree.ElementTree as ET

sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'scripts'))
from measure_drawing_profiles import trace_paths, polygon_mm, triangle_distance, point_segment_distance


class ProfileMeasurement(unittest.TestCase):
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
