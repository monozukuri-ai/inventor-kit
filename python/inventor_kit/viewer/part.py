"""Retain the complete body inventory while displaying an explicit selection."""
from .tessellation import IDENTITY, MeshBuilder


def body_nodes(result, body_ids):
    requested = set(result.report(body_ids=body_ids)["requested_body_ids"])
    return [dict(id=f"/document/body-{body.body_index}", parent=None,
                 name=f"Body {index + 1}", body_index=body.body_index, body_id=body.id,
                 source_sha256=result.source_sha256, candidate_id=result.candidate_id,
                 source=body.source or {}, mesh_id=None, kind="body",
                 status=("not_selected" if body.id not in requested else
                         "pending" if body.status == "converted_solid" else body.status),
                 reason=("Body was not selected" if body.id not in requested else
                         "; ".join(d.message for d in body.diagnostics) or None),
                 local_transform_mm=[list(r) for r in IDENTITY],
                 world_transform_mm=[list(r) for r in IDENTITY])
            for index, body in enumerate(result.bodies)]


def build_body_meshes(selected, nodes, directory, options):
    builder = MeshBuilder(directory, options)
    meshes = []
    by_id = {node["body_id"]: node for node in nodes}
    for body in selected:
        node = by_id[body.id]
        mesh_id = f"body-{body.body_index}"
        meshes.append(builder.add(body.shape, mesh_id))
        node.update(mesh_id=mesh_id, status="displayable", reason=None)
    return meshes, builder.settings()
