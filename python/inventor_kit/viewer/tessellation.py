"""Version-pinned OCP tessellation adapter. A missing face rejects the part."""
from importlib.metadata import version

QUALITY = {"draft": (0.3, 0.3), "normal": (0.1, 0.1), "fine": (0.03, 0.05)}


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


def build_meshes(shapes, doc, directory, options):
    import numpy as np
    from ocp_tessellate.tessellator import Tessellator, discretize_edge, get_edge_type
    from OCP.BRep import BRep_Tool

    linear, angular = QUALITY[options.quality]
    nodes, meshes = [], []
    total_bytes = total_triangles = 0
    source = doc.geometry.source_sha256
    selected = doc.geometry.selection.selected_id
    bodies = doc.model.bodies()
    if len(bodies) != len(shapes):
        raise ValueError("Body-to-shape mapping is ambiguous")
    for index, (shape, body) in enumerate(zip(shapes, bodies)):
        node_id = f"/document/body-{body.index}"
        tess = Tessellator(node_id)
        # This implementation's quality is an absolute OCCT deflection in mm.
        tess.compute(shape.wrapped, linear, angular, compute_edges=False)
        mesh = {key: getattr(tess, "get_" + key)().reshape(-1) for key in
                ("vertices", "triangles", "normals", "triangles_per_face", "face_types", "obj_vertices")}
        total_triangles += mesh["triangles"].size // 3
        if total_triangles > options.max_triangles:
            raise ValueError("Viewer triangle limit exceeded")
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
        for key in mesh:
            integer = key in ("triangles", "triangles_per_face", "face_types", "edge_types", "segments_per_edge")
            array = np.asarray(mesh[key], dtype="<u4" if integer else "<f4")
            total_bytes += array.nbytes
            if total_bytes > options.max_buffer_bytes:
                raise ValueError("Viewer buffer limit exceeded")
            resource = f"mesh-{index}-{key}.bin"
            (directory / resource).write_bytes(array.tobytes())
            buffers[key] = dict(resource=resource, dtype="uint32" if integer else "float32",
                                count=int(array.size), bytes=int(array.nbytes))
        points = mesh["vertices"].reshape(-1, 3)
        bb = {axis + suffix: float(fn(points[:, i])) for i, axis in enumerate("xyz")
              for suffix, fn in (("min", np.min), ("max", np.max))}
        mesh_id = f"mesh-{index}"
        meshes.append(dict(id=mesh_id, buffers=buffers, bounds=bb,
                           face_count=len(shape.Faces()), triangle_count=int(mesh["triangles"].size // 3)))
        nodes.append(dict(id=node_id, parent=None, name=f"Body {index + 1}", body_index=body.index,
                          source_sha256=source, candidate_id=selected, mesh_id=mesh_id))
    return nodes, meshes, dict(quality=options.quality, linear_deflection_mm=linear,
        angular_deflection_radians=angular, mode="absolute", triangle_count=int(total_triangles),
        buffer_bytes=int(total_bytes), packages={p: version(p) for p in ("ocp-tessellate", "cadquery", "cadquery-ocp")})
