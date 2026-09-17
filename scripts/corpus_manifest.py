"""Corpus split and source identity validation, independent of native imports."""
from pathlib import Path, PurePosixPath
import json
import re

ROOT = Path(__file__).resolve().parents[1]


def safe_relative(name):
    """Portable fixture/resource path, never an absolute path or alias."""
    if not isinstance(name, str) or not name or any(ord(c) < 32 or c in '\\:*?"<>|' for c in name):
        raise ValueError(f'Unsafe corpus path: {name!r}')
    parts = name.split('/')
    if any(p in ('', '.', '..') or p.endswith((' ', '.')) for p in parts):
        raise ValueError(f'Unsafe corpus path: {name!r}')
    if any(p.split('.')[0].upper() in {'CON', 'PRN', 'AUX', 'NUL',
           *[f'COM{i}' for i in range(1, 10)], *[f'LPT{i}' for i in range(1, 10)]} for p in parts):
        raise ValueError(f'Unsafe corpus path: {name!r}')
    path = PurePosixPath(name)
    if path.is_absolute():
        raise ValueError(f'Unsafe corpus path: {name!r}')
    return path


def validate_identities(rows):
    """Check shared identities across native, assembly and drawing manifests."""
    names, families, hashes = set(), {}, {}
    if not rows:
        raise ValueError('Empty corpus manifest')
    for row in rows:
        if not isinstance(row, dict) or not {'file', 'split', 'family_id', 'sha256', 'bytes'} <= row.keys():
            raise ValueError('Corpus asset lacks required identity fields')
        name, split, family = row['file'], row['split'], row['family_id']
        safe_relative(name)
        if name.casefold() in names:
            raise ValueError(f'Duplicate corpus path: {name}')
        names.add(name.casefold())
        if split not in ('regression', 'holdout') or not isinstance(family, str) or not family:
            raise ValueError(f'Invalid fixture split/family: {name}')
        if not isinstance(row['sha256'], str) or not re.fullmatch('[0-9a-f]{64}', row['sha256']) or type(row['bytes']) is not int or row['bytes'] <= 0:
            raise ValueError(f'Invalid fixture digest/size: {name}')
        for identity, seen in ((family, families), (row['sha256'], hashes)):
            if identity in seen and seen[identity] != split:
                raise ValueError(f'Family or duplicate data crosses corpus splits: {identity}')
            seen[identity] = split
    return rows


def validate_manifest(rows):
    validate_identities(rows)
    for row in rows:
        name, split = row['file'], row['split']
        if '/' in name:
            raise ValueError(f'Invalid fixture name: {name}')
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


def load_corpus(manifests):
    """Load canonical assets once. Drawing selections reference, not copy, rows."""
    rows, lookup = [], {}
    for path in map(Path, manifests):
        data = json.loads(path.read_text(encoding='utf-8'))
        if isinstance(data, dict):
            if data.get('schema_version') != 1 or not isinstance(data.get('assets'), list):
                raise ValueError(f'Unsupported corpus manifest: {path}')
            data = data['assets']
        if not isinstance(data, list) or not data:
            raise ValueError(f'Empty/invalid corpus manifest: {path}')
        validate_identities(data)
        for row in data:
            key = (path.name, row['file'])
            if key in lookup:
                raise ValueError(f'Duplicate manifest identity: {key}')
            lookup[key] = row
            rows.append(row)
    validate_identities(rows)
    return rows, lookup
