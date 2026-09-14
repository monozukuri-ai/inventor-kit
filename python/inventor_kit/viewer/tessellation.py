"""Version-pinned OCP tessellation adapter. A missing face rejects the part."""
from importlib.metadata import version

QUALITY = {"draft": (0.3, 0.3), "normal": (0.1, 0.1), "fine": (0.03, 0.05)}


def complete_cone_apex_normals(mesh, shape):
    """Fill only zero normals at proven cone apex nodes using the UV limit.

    The pinned tessellator evaluates a zero cross product at some exact cone
    apexes. Its duplicated UV nodes retain the approach angle, so the analytic
    limiting normal is defined per node. Positions and triangles stay intact.
    """
    import math
    import numpy as np
    from OCP.BRep import BRep_Tool
    from OCP.BRepAdaptor import BRepAdaptor_Surface
    from OCP.BRepGProp import BRepGProp_Face
    from OCP.BRepTools import BRepTools
    from OCP.GeomAbs import GeomAbs_Cone
    from OCP.TopLoc import TopLoc_Location
    from OCP.gp import gp_Pnt, gp_Vec

    normals = mesh['normals'].reshape(-1, 3)
    points = mesh['vertices'].reshape(-1, 3)
    if len(normals) != len(points):
        return 0  # The normal-buffer validation below will reject this mesh.
    missing = np.all(normals == 0, axis=1)
    if not np.any(missing):
        return 0
    offset = completed = 0
    for face in shape.Faces():
        location = TopLoc_Location()
        poly = BRep_Tool.Triangulation_s(face.wrapped, location)
        if poly is None:
            raise ValueError('Cannot map missing normals to a B-rep face')
        end = offset + poly.NbNodes()
        if end > len(normals):
            raise ValueError('Cannot map missing normals to mesh vertices')
        indices = np.flatnonzero(missing[offset:end])
        adaptor = BRepAdaptor_Surface(face.wrapped)
        if len(indices) and adaptor.GetType() == GeomAbs_Cone and poly.HasUVNodes():
            cone = adaptor.Cone()
            _, _, vmin, vmax = BRepTools.UVBounds_s(face.wrapped)
            angle, radius = cone.SemiAngle(), cone.RefRadius()
            sine, cosine = math.sin(angle), math.cos(angle)
            precision = BRep_Tool.Tolerance_s(face.wrapped)
            radii = (radius+vmin*sine, radius+vmax*sine)
            if (not math.isfinite(vmin+vmax) or min(radii) < -precision
                    or max(radii) <= precision):
                offset = end
                continue  # An ambiguous or negative nappe is not qualified.
            x = np.asarray(cone.Position().XDirection().Coord())
            y = np.asarray(cone.Position().YDirection().Coord())
            axis = np.asarray(cone.Axis().Direction().Coord())
            prop = BRepGProp_Face(face.wrapped)
            p, reference = gp_Pnt(), gp_Vec()
            for index in indices:
                u, v = poly.UVNode(int(index)+1).Coord()
                node = poly.Node(int(index)+1).Transformed(location.Transformation())
                if (abs(radius+v*sine) > precision or node.Distance(cone.Apex()) > precision):
                    continue
                if not np.array_equal(points[offset+index], np.asarray(node.Coord(), dtype=points.dtype)):
                    raise ValueError('Cone apex normal does not match the mesh vertex')
                normal = cosine*(math.cos(u)*x + math.sin(u)*y) - sine*axis
                # Orient the limit using a regular point on the same nappe.
                # The analytic cone normal depends on U, not on distance to apex.
                prop.Normal(u, (vmin+vmax)/2, p, reference)
                if reference.SquareMagnitude() == 0:
                    continue
                reference.Normalize()
                dot = float(np.dot(normal, reference.Coord()))
                if abs(abs(dot)-1.) > 1e-10:
                    continue
                normals[offset+index] = normal if dot > 0 else -normal
                completed += 1
        offset = end
    if offset != len(normals):
        raise ValueError('Cannot map missing normals to all mesh vertices')
    return completed


def validate_mesh(mesh, shape):
    import numpy as np
    from OCP.BRep import BRep_Tool
    from OCP.TopLoc import TopLoc_Location

    vertices, triangles, normals = (mesh[k] for k in ("vertices", "triangles", "normals"))
    if not vertices.size or vertices.size % 3 or not triangles.size or triangles.size % 3:
        raise ValueError("Empty or malformed triangulation")
    if triangles.min() < 0 or triangles.max() >= vertices.size // 3:
        raise ValueError("Triangle index outside the vertex buffer")
    if normals.size != vertices.size or mesh["edges"].size % 6:
        raise ValueError("Malformed normal or edge buffer")
    for key in ("vertices", "normals", "edges"):
        if not np.isfinite(mesh[key]).all():
            raise ValueError(f"Non-finite {key}")
    if np.any(np.linalg.norm(normals.reshape(-1, 3), axis=1) < 0.5):
        raise ValueError("Missing vertex normal")
    counts = mesh["triangles_per_face"]
    faces = shape.Faces()
    if len(counts) != len(faces) or np.any(counts <= 0) or counts.sum() != triangles.size // 3:
        raise ValueError("Tessellation omitted a B-rep face")
    for face in faces:
        poly = BRep_Tool.Triangulation_s(face.wrapped, TopLoc_Location())
        if poly is None or poly.NbTriangles() == 0:
            raise ValueError("B-rep face has no triangulation")


class MeshBuilder:
    """One session budget, with each distinct definition tessellated once."""
    def __init__(self, directory, options):
        self.directory, self.options = directory, options
        self.total_bytes = self.total_triangles = 0
        self.cone_apex_normal_count = 0

    def add(self, shape, mesh_id):
        import numpy as np
        from ocp_tessellate.tessellator import Tessellator, discretize_edge, get_edge_type
        from OCP.BRep import BRep_Tool

        if not shape.isValid():
            raise ValueError("Saved geometry did not produce a valid shape")
        linear, angular = QUALITY[self.options.quality]
        tess = Tessellator(mesh_id)
        # This implementation's quality is an absolute OCCT deflection in mm.
        tess.compute(shape.wrapped, linear, angular, compute_edges=False)
        mesh = {key: getattr(tess, "get_" + key)().reshape(-1) for key in
                ("vertices", "triangles", "normals", "triangles_per_face", "face_types", "obj_vertices")}
        triangles = int(mesh["triangles"].size // 3)
        if self.total_triangles + triangles > self.options.max_triangles:
            raise ValueError("Viewer triangle limit exceeded")
        self.cone_apex_normal_count += complete_cone_apex_normals(mesh, shape)
        edges, edge_types, edge_counts = [], [], []
        for edge in shape.Edges():
            if BRep_Tool.Degenerated_s(edge.wrapped):
                continue
            segments = np.asarray(discretize_edge(edge.wrapped, linear / 10)).reshape(-1)
            if not segments.size or segments.size % 6:
                raise ValueError("B-rep edge could not be discretized")
            edges.append(segments)
            edge_types.append(get_edge_type(edge.wrapped))
            edge_counts.append(segments.size // 6)
        mesh["edges"] = np.concatenate(edges) if edges else np.empty(0)
        mesh["edge_types"] = np.asarray(edge_types)
        mesh["segments_per_edge"] = np.asarray(edge_counts)
        validate_mesh(mesh, shape)
        buffers = {}
        arrays = {}
        for key in mesh:
            integer = key in ("triangles", "triangles_per_face", "face_types", "edge_types", "segments_per_edge")
            array = np.asarray(mesh[key], dtype="<u4" if integer else "<f4")
            if not integer and not np.isfinite(array).all():
                raise ValueError(f"Non-finite display buffer: {key}")
            arrays[key] = array
            if self.total_bytes + sum(a.nbytes for a in arrays.values()) > self.options.max_buffer_bytes:
                raise ValueError("Viewer buffer limit exceeded")
        for key, array in arrays.items():
            resource = f"{mesh_id}-{key}.bin"
            (self.directory / resource).write_bytes(array.tobytes())
            integer = array.dtype.kind == "u"
            buffers[key] = dict(resource=resource, dtype="uint32" if integer else "float32",
                                count=int(array.size), bytes=int(array.nbytes))
        points = mesh["vertices"].reshape(-1, 3)
        bb = {axis + suffix: float(fn(points[:, i])) for i, axis in enumerate("xyz")
              for suffix, fn in (("min", np.min), ("max", np.max))}
        self.total_bytes += sum(a.nbytes for a in arrays.values())
        self.total_triangles += triangles
        return dict(id=mesh_id, buffers=buffers, bounds=bb, face_count=len(shape.Faces()), triangle_count=triangles)

    def settings(self):
        linear, angular = QUALITY[self.options.quality]
        return dict(quality=self.options.quality, linear_deflection_mm=linear,
            angular_deflection_radians=angular, mode="absolute", triangle_count=self.total_triangles,
            buffer_bytes=self.total_bytes, packages={p: version(p) for p in ("ocp-tessellate", "cadquery", "cadquery-ocp")})


IDENTITY = ((1., 0., 0., 0.), (0., 1., 0., 0.), (0., 0., 1., 0.), (0., 0., 0., 1.))


def build_meshes(shapes, doc, directory, options):
    nodes, meshes = [], []
    builder = MeshBuilder(directory, options)
    bodies = doc.model.bodies()
    if len(bodies) != len(shapes):
        raise ValueError("Body-to-shape mapping is ambiguous")
    for index, (shape, body) in enumerate(zip(shapes, bodies)):
        node_id, mesh_id = f"/document/body-{body.index}", f"mesh-{index}"
        meshes.append(builder.add(shape, mesh_id))
        nodes.append(dict(id=node_id, parent=None, name=f"Body {index + 1}", body_index=body.index,
                          source_sha256=doc.geometry.source_sha256, candidate_id=doc.geometry.selection.selected_id,
                          mesh_id=mesh_id, kind="body", status="displayable", reason=None,
                          local_transform_mm=[list(r) for r in IDENTITY], world_transform_mm=[list(r) for r in IDENTITY]))
    return nodes, meshes, builder.settings()
