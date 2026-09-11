"""Adapt the existing IAM resolver/converter without flattening occurrences."""
from dataclasses import asdict
import hashlib
import math
from pathlib import Path, PureWindowsPath

from .. import _file_bytes, read_assembly_file
from ..assembly import AssemblyConversionError
from .scene import diagnostic, discard_geometry
from .tessellation import IDENTITY, MeshBuilder


def multiply(a, b):
    return tuple(tuple(sum(a[i][k] * b[k][j] for k in range(4)) for j in range(4)) for i in range(4))


def same_matrix(a, b):
    return a is not None and b is not None and all(
        math.isclose(a[i][j], b[i][j], rel_tol=1e-12, abs_tol=1e-8) for i in range(4) for j in range(4))


def rigid(matrix):
    if matrix is None or len(matrix) != 4 or any(len(r) != 4 for r in matrix):
        return False
    if not all(math.isfinite(v) for r in matrix for v in r) or tuple(matrix[3]) != IDENTITY[3]:
        return False
    for i in range(3):
        for j in range(3):
            if abs(sum(matrix[k][i] * matrix[k][j] for k in range(3)) - (i == j)) > 1e-10:
                return False
    a, b, c = (r[:3] for r in matrix[:3])
    det = a[0]*(b[1]*c[2]-b[2]*c[1]) - a[1]*(b[0]*c[2]-b[2]*c[0]) + a[2]*(b[0]*c[1]-b[1]*c[0])
    return abs(det - 1) <= 1e-10


def inventory(graph):
    nodes = []
    for index, instance in enumerate(graph.instances):
        owner = graph.definitions[instance.owner_definition]
        saved = owner.document.occurrences[instance.occurrence_index]
        reference = next((r for r in owner.document.references if r['reference_id'] == saved.reference_id), {})
        definition = graph.definitions[instance.definition] if instance.definition is not None else None
        name = instance.name or (Path(definition.path).stem if definition else PureWindowsPath(reference.get('path', '')).stem)
        parent = None if instance.parent is None else nodes[instance.parent]
        node_id = '/document/' + '/'.join(f'occ-{p}' for p in instance.path)
        if (parent is not None and tuple(parent['occurrence_path']) != instance.path[:-1]) or (parent is None and len(instance.path) != 1):
            raise ValueError('Occurrence path and parent disagree')
        local, world = instance.local_transform_mm, instance.world_transform_mm
        parent_world = IDENTITY if parent is None else parent['world_transform_mm']
        placement_ok = rigid(local) and rigid(world) and rigid(parent_world) and same_matrix(multiply(parent_world, local), world)
        status, reason = 'pending', 'Saved placement requires --allow-unverified-state'
        if instance.resolution != 'resolved':
            status, reason = instance.resolution, 'Reference or placement is unresolved'
        elif not placement_ok:
            status, reason = 'placement_unavailable', 'Unknown, non-rigid, mirrored, or inconsistent placement'
        if instance.suppressed is True:
            status, reason = 'suppressed', 'Stored reference suppression flag'
        elif instance.visible is False:
            status, reason = 'hidden', 'Occurrence visibility is false'
        elif instance.substitute is True:
            status, reason = 'unsupported_substitute', 'Substitute selection is unsupported'
        nodes.append(dict(id=node_id, parent=None if parent is None else parent['id'],
            name=name or f'Occurrence {instance.occurrence_id}', kind=definition.document.summary['kind'] if definition else 'unresolved',
            body_index=None, source_sha256=None if definition is None else definition.document.summary['source_sha256'],
            candidate_id=None, mesh_id=None, definition_key=None if definition is None else definition.key,
            instance_index=index, occurrence_path=list(instance.path), status=status, reason=reason,
            local_transform_mm=None if local is None else [list(r) for r in local],
            world_transform_mm=None if world is None else [list(r) for r in world],
            suppressed=instance.suppressed, visible=instance.visible, substitute=instance.substitute,
            source=dict(owner_definition=owner.key, occurrence=asdict(saved), reference=reference)))
    if len({n['id'] for n in nodes}) != len(nodes):
        raise ValueError('Duplicate occurrence identity')
    return nodes


def verify_sources(graph):
    # The converter verifies part inputs. Also protect assembly placements and
    # every source alias, including changes during tessellation.
    for definition in graph.definitions:
        for path in set((definition.path, *definition.source_paths)):
            if hashlib.sha256(_file_bytes(Path(path), graph.limits)).hexdigest() != definition.document.summary['source_sha256']:
                raise ValueError(f'Source file changed after assembly resolution: {path}')


def converted_shapes(conversion, nodes):
    """Use converter metadata for identity and check its actual placement chain.

    Leaf ``obj`` stays in definition coordinates (including its intrinsic shape
    location). Assembly node locations are applied only by the browser hierarchy.
    """
    by_path = {tuple(n['occurrence_path']): n for n in nodes}
    shapes, found = {}, set()

    def visit(item, parent_world):
        trsf = item.loc.wrapped.Transformation()
        local = tuple(tuple(trsf.Value(i+1, j+1) for j in range(4)) for i in range(3)) + (IDENTITY[3],)
        world = multiply(parent_world, local)
        if item.obj is not None:
            path = tuple(item.metadata.get('occurrence_path', ()))
            node = by_path.get(path)
            if node is None or path in found or node['definition_key'] != item.metadata.get('definition_key'):
                raise ValueError('Converted shape has ambiguous occurrence metadata')
            if node['status'] == 'placement_unavailable':
                return
            if not same_matrix(world, node['world_transform_mm']):
                raise ValueError('Converted occurrence placement differs from the saved graph')
            found.add(path)
            shapes.setdefault(node['definition_key'], item.obj)
        for child in item.children:
            visit(child, world)

    visit(conversion.assembly, IDENTITY)
    return shapes, found


def omit(scene, node, reason, detail):
    node.update(mesh_id=None, status=reason, reason=detail)
    scene['omissions'].append(dict(instance=node['instance_index'], path=node['occurrence_path'], reason=reason, detail=detail))


def build_assembly_scene(path, directory, options, scene, publish):
    stage = 'geometry'
    try:
        graph = read_assembly_file(path, search_roots=options.search_roots, limits=options.limits)
        root = graph.definitions[graph.summary['root']]
        if root.document.summary['source_sha256'] != scene['source']['sha256']:
            raise ValueError('Root file changed between document inspection and assembly resolution')
        scene['assembly'] = dict(structure_status=graph.structure_status, current_state='unverified', complete=False,
            allow_unverified_state=options.allow_unverified_state, allow_partial=options.allow_partial,
            converted_instances=0, converted_definitions=0, displayed_instances=0, displayed_definitions=0,
            source_documents=[dict(key=d.key, path=d.path, source_paths=d.source_paths,
                sha256=d.document.summary['source_sha256'], kind=d.document.summary['kind']) for d in graph.definitions])
        scene['nodes'] = inventory(graph)
        scene['reference_issues'] = [dict(owner=d.key, **r) for d in graph.definitions for r in d.references if r['status'] != 'resolved']
        for d in graph.definitions:
            scene['diagnostics'].extend(d.document.diagnostics)
        for message in graph.summary['diagnostics']:
            diagnostic(scene, 'viewer.assembly_graph', message, 'warning')
        scene['stages'][stage] = graph.structure_status
        publish(scene)
        if not options.allow_unverified_state:
            for node in scene['nodes']:
                if node['status'] == 'pending':
                    node['status'] = 'state_unverified'
            diagnostic(scene, 'viewer.assembly_state_unverified',
                'Saved IAM placements require --allow-unverified-state. References and occurrences are available for inspection.', 'warning')
            return scene

        stage = 'conversion'
        refused = False
        try:
            result = graph.to_cadquery(allow_unverified_state=options.allow_unverified_state, allow_partial=options.allow_partial)
        except AssemblyConversionError as error:
            result, refused = error.result, True
            diagnostic(scene, 'viewer.assembly_partial_disallowed',
                'The saved assembly is incomplete. Use --allow-partial to display the available parts.',
                source={'conversion_message': str(error)})
        scene['assembly'].update(converted_instances=result.converted_instances, converted_definitions=result.converted_definitions)
        for omission in result.omissions:
            omit(scene, scene['nodes'][omission.instance], omission.reason, omission.detail)
        # Keep the converter's reference issues and diagnostics, including entries
        # that cannot be attached to a decoded occurrence.
        scene['reference_issues'] = list(result.reference_issues)
        for message in result.diagnostics:
            diagnostic(scene, 'viewer.assembly_conversion', message, 'warning')
        verify_sources(graph)
        if refused:
            scene['stages'][stage] = 'partial_disallowed'
            discard_geometry(scene, 'partial_disallowed', 'Incomplete assembly requires --allow-partial')
            return scene
        scene['stages'][stage] = 'available'
        publish(scene)
        shapes, converted = converted_shapes(result, scene['nodes'])
        for node in scene['nodes']:
            if node['status'] == 'placement_unavailable':
                omit(scene, node, 'placement_unavailable', node['reason'])
            elif node['status'] == 'pending':
                if node['kind'] == 'assembly':
                    node.update(status='group', reason=None)
                elif tuple(node['occurrence_path']) not in converted:
                    omit(scene, node, 'geometry_unavailable', 'No converted shape for this occurrence')
        stage = 'tessellation'
        builder = MeshBuilder(directory, options)
        meshes = {}
        for key, shape in shapes.items():
            members = [n for n in scene['nodes'] if n['definition_key'] == key and n['status'] == 'pending']
            if not members:
                continue
            try:
                mesh = builder.add(shape, f'assembly-mesh-{len(meshes)}')
                meshes[key] = mesh
                for node in members:
                    node.update(mesh_id=mesh['id'], status='displayable', reason=None)
            except Exception as error:
                for node in members:
                    omit(scene, node, 'tessellation_failed', str(error))
        if not options.allow_partial and any(o['reason'] not in ('suppressed', 'hidden') for o in scene['omissions']):
            scene['stages'][stage] = 'partial_disallowed'
            diagnostic(scene, 'viewer.assembly_partial_disallowed', 'Display mesh or placement is incomplete; use --allow-partial to view the available occurrences.')
            discard_geometry(scene, 'partial_disallowed', 'Incomplete assembly requires --allow-partial')
            return scene
        rendered_triangles = sum(meshes[n['definition_key']]['triangle_count'] for n in scene['nodes'] if n['mesh_id'])
        if rendered_triangles > options.max_triangles:
            raise ValueError('Viewer triangle limit exceeded after occurrence expansion')
        verify_sources(graph)
        scene['meshes'] = list(meshes.values())
        scene['tessellation'] = dict(builder.settings(), instanced_triangle_count=rendered_triangles)
        scene['assembly'].update(displayed_instances=sum(n['mesh_id'] is not None for n in scene['nodes']), displayed_definitions=len(meshes))
        scene['stages'][stage] = 'partial' if any(o['reason'] not in ('suppressed', 'hidden') for o in scene['omissions']) or graph.structure_status != 'resolved' else 'available'
    except Exception as error:
        scene['stages'][stage] = 'failed'
        diagnostic(scene, f'viewer.assembly_{stage}_failed', str(error))
        discard_geometry(scene, 'not_displayed', 'Assembly conversion or validation failed')
    finally:
        scene['job_status'] = 'finished'
    return scene
