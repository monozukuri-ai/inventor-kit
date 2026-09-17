"""Strict offline contract for drawing observations, not provider certification."""
import hashlib
import json
import math
from pathlib import Path

from jsonschema import Draft202012Validator, FormatChecker

from corpus_manifest import ROOT, safe_relative

MAX_JSON_BYTES = 32 * 1024 * 1024


def identity(path):
    path = Path(path)
    sha = hashlib.sha256()
    size = 0
    with path.open('rb') as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b''):
            sha.update(chunk)
            size += len(chunk)
    return dict(file_name=path.name, bytes=size, sha256=sha.hexdigest())


def read_json(path):
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                raise ValueError(f'Duplicate JSON key: {key}')
            result[key] = value
        return result
    def invalid(value):
        raise ValueError(f'Non-finite JSON number: {value}')
    with Path(path).open('rb') as source:
        payload = source.read(MAX_JSON_BYTES + 1)
    if len(payload) > MAX_JSON_BYTES:
        raise ValueError('Drawing JSON exceeds size limit')
    return json.loads(payload, object_pairs_hook=pairs, parse_constant=invalid)


def walk(value, path='$'):
    yield path, value
    if isinstance(value, dict):
        for key, child in value.items():
            yield from walk(child, path + '.' + key)
    elif isinstance(value, list):
        for index, child in enumerate(value):
            yield from walk(child, f'{path}[{index}]')


def validate_capture(data, *, source=None, artifact_root=None):
    schema = read_json(ROOT / 'schemas/drawing-oracle-v1.schema.json')
    Draft202012Validator(schema, format_checker=FormatChecker()).validate(data)
    for path, node in walk(data):
        if isinstance(node, float) and not math.isfinite(node):
            raise ValueError(f'Non-finite value: {path}')
        if not isinstance(node, dict) or 'reported_count' not in node:
            continue
        if node['status'] in ('captured', 'partial'):
            count, items = node['reported_count'], node['items']
            if count != len(items) or [v['index'] for v in items] != list(range(1, count + 1)):
                raise ValueError(f'Collection count/order mismatch: {path}')
            complete = all(v['result']['status'] == 'captured' for v in items)
            if (node['status'] == 'captured') != complete:
                raise ValueError(f'Collection status hides failed items: {path}')
    native = data['provider']['name'] != 'synthetic'
    if native != (data['capture']['scope'] == 'opened_document'):
        raise ValueError('Synthetic/native provider and scope disagree')
    if native and data['provider']['platform'] != 'Windows':
        raise ValueError('Native provider must identify Windows')
    if data['capture']['automatic_update'] != 'unknown' and data['capture']['state_evidence']['status'] != 'captured':
        raise ValueError('Known update state needs evidence')
    if data['capture']['state_evidence'].get('value') is not None and not data['capture']['state_evidence']['value'].strip():
        raise ValueError('Empty saved-state evidence')
    for state in (data['document_state_before'], data['document_state_after']):
        if state['document_type'].get('value', 12292) != 12292:
            raise ValueError('Provider did not identify a drawing document')
    for sheet in data['sheets']['items']:
        if sheet['result']['status'] == 'captured':
            for name in ('width', 'height'):
                field = sheet['result']['value'][name]
                if field['status'] == 'captured' and field['value'] <= 0:
                    raise ValueError('Sheet dimensions must be positive')
    names = set()
    for item in data['input_files']:
        safe_relative(item['path'])
        if item['path'].casefold() in names or Path(item['path']).name != item['identity']['file_name']:
            raise ValueError('Duplicate/invalid staged input identity')
        names.add(item['path'].casefold())
    if data['source'] not in [v['identity'] for v in data['input_files']]:
        raise ValueError('Source absent from staged input identities')
    staged_identities = [v['identity'] for v in data['input_files']]
    for reference in data['references']['items']:
        if reference['result']['status'] == 'captured':
            result = reference['result']['value']['identity']
            if result['status'] == 'captured' and result['value'] not in staged_identities:
                raise ValueError('Reference identity absent from staged inputs')
    if source is not None:
        source = Path(source).resolve()
        if identity(source) != data['source']:
            raise ValueError('Capture source identity mismatch')
        matches = [v for v in data['input_files'] if v['identity'] == data['source']]
        if len(matches) != 1:
            raise ValueError('Ambiguous source path in staged inputs')
        relative = Path(matches[0]['path'])
        # Recover the preserved project root from the source's recorded relative
        # path, then bind every copied dependency to the local source tree.
        root = source
        for _ in relative.parts:
            root = root.parent
        if root / relative != source:
            raise ValueError('Source relative path does not match the local project')
        for entry in data['input_files']:
            target = (root / entry['path']).resolve()
            if not target.is_relative_to(root) or identity(target) != entry['identity']:
                raise ValueError('Staged input identity mismatch')
    artifacts = set()
    sheet_count = data['sheets']['reported_count']
    for item in data['visual_artifacts']:
        safe_relative(item['path'])
        if item['path'].casefold() in artifacts:
            raise ValueError('Duplicate visual artifact')
        artifacts.add(item['path'].casefold())
        if item['source_sha256'] != data['source']['sha256']:
            raise ValueError('Artifact source hash mismatch')
        if item['sheet_index'] is not None and (sheet_count is None or item['sheet_index'] > sheet_count):
            raise ValueError('Artifact references an absent sheet')
        if artifact_root is not None:
            root = Path(artifact_root).resolve()
            target = (root / item['path']).resolve()
            if not target.is_relative_to(root):
                raise ValueError('Artifact path escapes root')
            actual = identity(target)
            if (actual['bytes'], actual['sha256']) != (item['bytes'], item['sha256']):
                raise ValueError('Artifact identity mismatch')
            with target.open('rb') as stream:
                signature = stream.read(8)
            if not (signature.startswith(b'%PDF-') if item['media_type'] == 'application/pdf' else signature == b'\x89PNG\r\n\x1a\n'):
                raise ValueError('Artifact media signature mismatch')
    return data


def readiness(data):
    """Conservative acquisition gate; never an IDW decoder/display correctness gate."""
    reasons = []
    if data['provider']['name'] == 'synthetic':
        reasons.append('synthetic_provider')
    if data['capture']['automatic_update'] == 'unknown':
        reasons.append('saved_state_not_verified')
    elif data['capture']['automatic_update'] == 'occurred':
        reasons.append('opened_state_differs_from_saved_state')
    if data['sheets']['status'] != 'captured' or not data['sheets']['items']:
        reasons.append('no_complete_sheets')
    for path, node in walk(data):
        if isinstance(node, dict) and node.get('status') in ('unavailable', 'failed', 'partial'):
            reasons.append(path + ':' + node['status'])
    for item in data['references']['items']:
        if item['result']['status'] == 'captured' and item['result']['value']['missing'].get('value') is True:
            reasons.append('missing_reference')
    # Failed enumeration can retain a count above the collector budget; never
    # allocate or iterate from that untrusted count.
    for sheet in data['sheets']['items']:
        index = sheet['index']
        if not any(a['binding'] == 'same_session' and a['sheet_index'] == index for a in data['visual_artifacts']):
            reasons.append(f'sheet_{index}_visual_not_bound')
    return dict(status='ready' if not reasons else 'incomplete', reasons=reasons)


def load_capture(path, **kwargs):
    return validate_capture(read_json(path), **kwargs)
