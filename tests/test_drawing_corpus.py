import copy
import hashlib
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
from check_corpus import verify
from corpus_manifest import load_corpus, safe_relative, validate_identities
from drawing_corpus import drawing_rows, load_drawings
from fetch_drawing_samples import fetch


class DrawingCorpus(unittest.TestCase):
    def row(self, name='one.idw', split='regression', family='one', payload=b'fixture'):
        return dict(file=name, split=split, family_id=family, bytes=len(payload),
                    sha256=hashlib.sha256(payload).hexdigest(), url='https://fixtures.example/'+name)

    def test_real_manifest_reserves_distinct_families_and_reuses_existing_identities(self):
        data, lookup = load_drawings()
        rows = drawing_rows()
        self.assertEqual(len(rows), 11)
        existing = [d for d, r in rows if d['fixture']['manifest'] == 'manifest.json']
        self.assertEqual(len(existing), 2)
        self.assertFalse({d['fixture']['file'] for d in existing} & {r['file'] for r in data['assets']})
        held = {d['role']: r['family_id'] for d, r in rows if r['split'] == 'holdout'}
        self.assertEqual(set(held), {'part', 'assembly'})
        self.assertEqual(len(set(held.values())), 2)
        self.assertTrue(all(d['oracle']['status'] == 'not_collected' for d, r in rows))
        self.assertTrue(all(v['binding'] == 'unverified' for d, r in rows for v in d['visuals']))

    def test_cross_manifest_family_and_duplicate_hash_leakage(self):
        for same_family in (True, False):
            with self.subTest(same_family=same_family), tempfile.TemporaryDirectory() as directory:
                a = self.row()
                b = self.row('nested/two.idw', 'holdout', 'one' if same_family else 'two', b'other' if same_family else b'fixture')
                paths = [Path(directory)/n for n in ('native.json', 'drawing.json')]
                paths[0].write_text(json.dumps([a]))
                paths[1].write_text(json.dumps(dict(schema_version=1, assets=[b])))
                with self.assertRaisesRegex(ValueError, 'crosses corpus splits'):
                    load_corpus(paths)

    def test_paths_and_case_aliases_are_rejected(self):
        for name in ('../outside', '/outside', 'C:/outside', 'a\\b', 'a//b', './a', 'a/../b', 'a. ', 'a\0b', 'CON.idw', 'a/b?', 'a\nb'):
            with self.subTest(name=name), self.assertRaises(ValueError):
                safe_relative(name)
        with self.assertRaisesRegex(ValueError, 'Duplicate corpus path'):
            validate_identities([self.row('A.idw'), self.row('a.idw')])

    def test_corpus_verifier_checks_size_and_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest = root / 'manifest.json'
            manifest.write_text(json.dumps([self.row()]))
            with self.assertRaisesRegex(ValueError, 'Missing'):
                verify(root, [manifest])
            (root/'one.idw').write_bytes(b'changed')
            with self.assertRaisesRegex(ValueError, 'mismatch'):
                verify(root, [manifest])
            (root/'one.idw').write_bytes(b'fixture')
            self.assertEqual(verify(root, [manifest])['fixtures'], 1)

    def test_drawing_download_is_verified_and_cached(self):
        row = self.row('drawing/one.idw')
        data = dict(assets=[row], drawings=[dict(fixture=dict(manifest='native.json', file=row['file']))])
        for payload in (b'changed', b'fixture too big', b'fixture'):
            with self.subTest(payload=payload), tempfile.TemporaryDirectory() as directory:
                with patch('fetch_drawing_samples.load_drawings', return_value=(data, {('native.json',row['file']): row})), \
                     patch('fetch_drawing_samples.download', return_value=payload) as download:
                    if payload != b'fixture':
                        with self.assertRaisesRegex(ValueError, 'size/hash mismatch'):
                            fetch(directory)
                        self.assertFalse((Path(directory)/row['file']).exists())
                    else:
                        fetch(directory)
                        fetch(directory)
                        self.assertEqual(download.call_count, 1)
                        self.assertEqual((Path(directory)/row['file']).read_bytes(), payload)

    def test_invalid_selections_cannot_bypass_canonical_manifest(self):
        source = json.loads((ROOT/'fixtures/drawing-manifest.json').read_text())
        changes = [lambda d: d['drawings'][0]['fixture'].update(file='absent.idw'),
                   lambda d: d['drawings'][0]['oracle'].update(status='captured'),
                   lambda d: d['drawings'][-1]['references'].append('drawings/rim/RIM.ipt'),
                   lambda d: d['drawings'][-1]['visuals'][0].update(binding='same_session')]
        for change in changes:
            with tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                for name in ('manifest.json', 'assembly-manifest.json'):
                    (root/name).write_bytes((ROOT/'fixtures'/name).read_bytes())
                value = copy.deepcopy(source)
                change(value)
                (root/'drawing-manifest.json').write_text(json.dumps(value))
                with self.assertRaises(ValueError):
                    load_drawings(root)


if __name__ == '__main__':
    unittest.main()
