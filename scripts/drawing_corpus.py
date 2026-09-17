"""Drawing selections, canonical fixture identities and reserved validation data."""
import json
from pathlib import Path

from corpus_manifest import ROOT, load_corpus, safe_relative


def load_drawings(fixtures_dir=ROOT / 'fixtures'):
    fixtures_dir = Path(fixtures_dir)
    manifest = fixtures_dir / 'drawing-manifest.json'
    data = json.loads(manifest.read_text(encoding='utf-8'))
    if set(data) != {'schema_version', 'assets', 'drawings'} or data['schema_version'] != 1:
        raise ValueError('Invalid drawing manifest envelope')
    rows, lookup = load_corpus([fixtures_dir / name for name in
        ('manifest.json', 'assembly-manifest.json', 'drawing-manifest.json')])
    by_file = {r['file']: r for r in rows}
    seen, selections = set(), set()
    if not data['drawings']:
        raise ValueError('Empty drawing selection')
    for drawing in data['drawings']:
        if set(drawing) != {'id', 'fixture', 'role', 'role_basis', 'references', 'reference_completeness', 'visuals', 'oracle'}:
            raise ValueError('Invalid drawing selection fields')
        safe_relative(drawing['id'])
        if '/' in drawing['id'] or drawing['id'] in seen:
            raise ValueError('Duplicate/invalid drawing ID')
        seen.add(drawing['id'])
        ref = drawing['fixture']
        if set(ref) != {'manifest', 'file'} or (ref['manifest'], ref['file']) not in lookup:
            raise ValueError('Drawing references an absent canonical fixture')
        source = lookup[ref['manifest'], ref['file']]
        if not source['file'].lower().endswith('.idw') or source['sha256'] in selections:
            raise ValueError('Duplicate/non-IDW drawing selection')
        selections.add(source['sha256'])
        if drawing['role'] not in ('part', 'assembly', 'template', 'unknown') or not drawing['role_basis']:
            raise ValueError('Drawing role needs an evidence basis')
        if drawing['reference_completeness'] != 'unverified':
            raise ValueError('Reference completeness has not been qualified')
        if len(drawing['references']) != len(set(drawing['references'])):
            raise ValueError('Duplicate drawing reference')
        for name in drawing['references']:
            if name not in by_file or not name.lower().endswith(('.ipt', '.iam', '.ipn')):
                raise ValueError('Absent/non-native drawing reference')
            if (by_file[name]['family_id'], by_file[name]['split']) != (source['family_id'], source['split']):
                raise ValueError('Drawing references cross family/split boundaries')
        for visual in drawing['visuals']:
            if set(visual) != {'file', 'binding', 'basis'} or visual['binding'] != 'unverified' or not visual['basis']:
                raise ValueError('Upstream visuals are not authenticated native captures')
            asset = by_file.get(visual['file'])
            if asset is None or asset.get('asset_kind') != 'visual_reference':
                raise ValueError('Unknown visual reference')
            if (asset['family_id'], asset['split']) != (source['family_id'], source['split']):
                raise ValueError('Visual reference crosses family/split boundaries')
        oracle = drawing['oracle']
        if oracle != {'status': 'not_collected'}:
            raise ValueError('Attach captures by source SHA-256; manifest must not claim acquisition')
    for asset in data['assets']:
        if asset.get('asset_kind') not in ('native_drawing', 'native_reference', 'visual_reference', 'license'):
            raise ValueError('Unknown drawing asset kind')
        if not asset.get('license_notice') or not asset.get('license_url'):
            raise ValueError('Drawing asset lacks provenance/license notice')
        if asset['split'] == 'holdout' and (asset.get('baseline') is not None or not asset.get('holdout_policy')):
            raise ValueError('Holdout asset must remain unfitted')
    return data, lookup


def drawing_rows(fixtures_dir=ROOT / 'fixtures'):
    data, lookup = load_drawings(fixtures_dir)
    return [(entry, lookup[entry['fixture']['manifest'], entry['fixture']['file']]) for entry in data['drawings']]
