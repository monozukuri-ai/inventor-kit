"""Corpus split and source identity validation, independent of native imports."""
from pathlib import Path
import json
import re

ROOT = Path(__file__).resolve().parents[1]


def validate_manifest(rows):
    names, families, hashes = set(), {}, {}
    if not rows:
        raise ValueError('Empty corpus manifest')
    for row in rows:
        name, split, family = row['file'], row['split'], row['family_id']
        if not name or any(c in name for c in '/\\\0') or name in ('.', '..') or name in names:
            raise ValueError(f'Invalid or duplicate fixture name: {name}')
        names.add(name)
        if split not in ('regression', 'holdout') or not isinstance(family, str) or not family:
            raise ValueError(f'Invalid fixture split/family: {name}')
        if not re.fullmatch('[0-9a-f]{64}', row['sha256']) or type(row['bytes']) is not int or row['bytes'] <= 0:
            raise ValueError(f'Invalid fixture digest/size: {name}')
        for identity, seen in ((family, families), (row['sha256'], hashes)):
            if identity in seen and seen[identity] != split:
                raise ValueError(f'Family or duplicate data crosses corpus splits: {identity}')
            seen[identity] = split
        if split == 'holdout' and (row.get('baseline') is not None or not row.get('holdout_policy')):
            raise ValueError('Holdouts must have an explicit policy and no fitted baseline')
        if split == 'regression' and not row.get('baseline'):
            raise ValueError(f'Regression fixture lacks baseline: {name}')
    return rows


def load_manifest(path=ROOT / 'fixtures/manifest.json', split='all'):
    rows = validate_manifest(json.loads(Path(path).read_text()))
    if split not in ('all', 'regression', 'holdout'):
        raise ValueError(f'Invalid corpus split: {split}')
    return [row for row in rows if split == 'all' or row['split'] == split]
