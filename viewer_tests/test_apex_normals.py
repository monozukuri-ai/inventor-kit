"""Cone apex shading has a known limit; geometry and unknown failures stay intact."""
import math
import unittest

import cadquery as cq
import numpy as np
from OCP.BRep import BRep_Tool
from OCP.BRepAdaptor import BRepAdaptor_Surface
from OCP.GeomAbs import GeomAbs_Cone
from OCP.TopLoc import TopLoc_Location
from ocp_tessellate.tessellator import Tessellator

from inventor_kit.viewer.tessellation import complete_cone_apex_normals, validate_mesh


def tessellate(shape):
    tess = Tessellator('synthetic-cone')
    tess.compute(shape.wrapped, .1, .1, compute_edges=False)
    mesh = {k: getattr(tess, 'get_'+k)().reshape(-1) for k in
            ('vertices', 'triangles', 'normals', 'triangles_per_face')}
    mesh['edges'] = np.empty(0)
    return mesh


class ApexNormals(unittest.TestCase):
    def test_cone_limit_is_oriented_and_only_missing_apex_normals_change(self):
        cone = cq.Solid.makeCone(3, 0, 4)
        side = next(f for f in cone.Faces() if BRepAdaptor_Surface(f.wrapped).GetType() == GeomAbs_Cone)
        for reverse in (False, True):
            for angle in (0, 37):
                face = side.rotate((0, 0, 0), (0, 1, 0), angle).translate((10, 20, 30))
                if reverse:
                    face = cq.Face(face.wrapped.Reversed())
                mesh = tessellate(face)
                location = TopLoc_Location()
                poly = BRep_Tool.Triangulation_s(face.wrapped, location)
                apex = BRepAdaptor_Surface(face.wrapped).Cone().Apex()
                indices = [i-1 for i in range(1, poly.NbNodes()+1)
                           if poly.Node(i).Transformed(location.Transformation()).Distance(apex) < 1e-8]
                self.assertTrue(indices)
                mesh['normals'].reshape(-1, 3)[indices] = 0.
                before = {k: a.copy() for k, a in mesh.items()}
                self.assertEqual(complete_cone_apex_normals(mesh, face), len(indices))
                for key in ('vertices', 'triangles', 'triangles_per_face', 'edges'):
                    np.testing.assert_array_equal(mesh[key], before[key])
                normals = mesh['normals'].reshape(-1, 3)
                unaffected = np.ones(len(normals), dtype=bool)
                unaffected[indices] = False
                np.testing.assert_array_equal(normals[unaffected], before['normals'].reshape(-1, 3)[unaffected])
                # A radius-3, height-4 cone has axial normal component 3/5,
                # independent of U. Reflection of face sense changes its sign.
                axis = np.array([math.sin(math.radians(angle)), 0., math.cos(math.radians(angle))])
                np.testing.assert_allclose(normals[indices] @ axis, -.6 if reverse else .6, atol=1e-7)
                np.testing.assert_allclose(np.linalg.norm(normals[indices], axis=1), 1., atol=1e-7)
                validate_mesh(mesh, face)

    def test_zero_normal_on_regular_face_and_bad_mapping_are_not_repaired(self):
        plane = cq.Face.makePlane(2, 3)
        mesh = tessellate(plane)
        mesh['normals'][:3] = 0.
        self.assertEqual(complete_cone_apex_normals(mesh, plane), 0)
        with self.assertRaisesRegex(ValueError, 'Missing vertex normal'):
            validate_mesh(mesh, plane)
        cone = cq.Solid.makeCone(3, 0, 4)
        side = next(f for f in cone.Faces() if BRepAdaptor_Surface(f.wrapped).GetType() == GeomAbs_Cone)
        mesh = tessellate(side)
        location = TopLoc_Location()
        poly = BRep_Tool.Triangulation_s(side.wrapped, location)
        apex = BRepAdaptor_Surface(side.wrapped).Cone().Apex()
        indices = [i-1 for i in range(1, poly.NbNodes()+1)
                   if poly.Node(i).Transformed(location.Transformation()).Distance(apex) < 1e-8]
        mesh['normals'].reshape(-1, 3)[indices] = 0.
        mesh['vertices'].reshape(-1, 3)[indices[0], 0] += .01
        with self.assertRaisesRegex(ValueError, 'does not match the mesh vertex'):
            complete_cone_apex_normals(mesh, side)


if __name__ == '__main__':
    unittest.main()
