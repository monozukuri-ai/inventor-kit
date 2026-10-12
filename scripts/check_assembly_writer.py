"""Check that IAM defaults are typed rules, independent of private CAD inputs."""
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def check(path):
    profile = json.loads(path.read_text(encoding='utf-8'))
    if set(profile) != {'schema', 'profile', 'graph', 'tables', 'ufrx_fields'}:
        raise ValueError('IAM defaults contain undeclared top-level data')
    if profile['schema'] != 1 or profile['profile'] != 'inventor-iam-rse31-fixed-parts-v1':
        raise ValueError('Unknown IAM defaults profile')
    counts = dict(AmDcSegment=37, AmGraphicsSegment=204, AmBrowserSegment=25,
                  AmAppSegment=201, NBNotebookSegment=2, AmRxSegment=10,
                  AmBREPSegment=6, DesignViewSegment=4)
    graph = profile['graph']
    if (graph['occurrence_count'], graph['reference_count']) != (1, 1):
        raise ValueError('IAM defaults must describe one expandable role layout')
    if {k: len(v) for k, v in graph['owners'].items()} != counts:
        raise ValueError('Incomplete IAM role layout')
    forbidden = {'payload', 'payload_hex', 'raw', 'source', 'source_id', 'sha256',
                 'bytes', 'path', 'timestamps', 'statistics', 'catalog', 'native_file'}
    def walk(value):
        if isinstance(value, dict):
            if forbidden & value.keys():
                raise ValueError('Private/opaque data in IAM defaults')
            for item in value.values():
                walk(item)
        elif isinstance(value, list):
            for item in value:
                walk(item)
        elif isinstance(value, str):
            if any(s in value for s in ('DrawingTEST', '/home/', 'C:\\Users\\', 'internal/', '.ipt')):
                raise ValueError('Source-specific data in IAM defaults')
    walk(profile)
    kinds = {'u8', 'u16', 'u32', 'i32', 'u48', 'f32', 'f64', 'ascii', 'utf16', 'guid'}
    policies = {'fixed', 'assigned-key', 'document-revision', 'document-state',
                'local-reference', 'registry-reference', 'document_id', 'revision_id', 'part-path'}
    fields = list(profile['ufrx_fields'])
    for owner, records in graph['owners'].items():
        roles = {r['role']: r for r in records}
        if len(roles) != len(records):
            raise ValueError('Duplicate IAM role')
        for record in records:
            recipe = record['recipe']
            if recipe['kind'] == 'empty-asm-carrier':
                if owner not in ('AmDcSegment', 'AmBREPSegment') or set(recipe) != {'kind', 'helper_role'}:
                    raise ValueError('Invalid empty carrier recipe')
            elif recipe['kind'] == 'fields':
                fields.extend(recipe['fields'])
                for field in recipe['fields']:
                    if field['policy'] == 'local-reference':
                        if roles[field['target_role']]['class'] != field['target_class']:
                            raise ValueError('IAM local target class differs')
                    if field['policy'] == 'registry-reference':
                        if field['target_node'] not in graph['registry_nodes'][owner]:
                            raise ValueError('Unresolved IAM registry target')
            else:
                raise ValueError('Unknown IAM record recipe')
    for field in fields:
        if field['kind'] not in kinds or field['policy'] not in policies:
            raise ValueError('Non-typed IAM field')
        if field['policy'] == 'document-state' and ('value' in field or field['kind'] != 'guid'):
            raise ValueError('Document state must be freshly allocated')
    return len(fields)


if __name__ == '__main__':
    count = check(ROOT/'crates/inventor-core/src/write/assembly/profile.json')
    print(f'IAM defaults passed: {count} typed fields; no source-file or opaque payload dependency')
