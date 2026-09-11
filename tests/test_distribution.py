"""Validate wheel RECORD paths and integrity independently of the build host."""
import base64
import csv
import hashlib
import io
from pathlib import Path
import sys
import tempfile
import tomllib
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
from check_distribution import check


class WheelRecords(unittest.TestCase):
    def setUp(self):
        self.version = tomllib.loads((ROOT / 'pyproject.toml').read_text())['project']['version']
        info = f'inventor_kit-{self.version}.dist-info'
        self.record = info + '/RECORD'
        # This synthetic wheel exercises archive validation, not native loading.
        self.contents = {
            'inventor_kit/_inventor.pyd': b'synthetic native extension',
            info + '/METADATA': (
                f'Metadata-Version: 2.4\nName: inventor-kit\nVersion: {self.version}\n'
                'License-Expression: MIT\nRequires-Python: >=3.11\n'
                'Requires-Dist: cq-acis>=0.3.2,<0.4\n\n'
            ).encode(),
            info + '/WHEEL': b'Wheel-Version: 1.0\nRoot-Is-Purelib: false\nTag: cp310-abi3-win_amd64\n',
        }
        self.contents[info+'/METADATA'] = self.contents[info+'/METADATA'].rstrip() + b'\nProvides-Extra: viewer\nRequires-Dist: ocp-tessellate==3.5.1; extra == "viewer"\n\n'
        for path in (ROOT/'python/inventor_kit/viewer').rglob('*'):
            if path.is_file() and (path.suffix == '.py' or 'static' in path.parts):
                self.contents['inventor_kit/viewer/'+path.relative_to(ROOT/'python/inventor_kit/viewer').as_posix()] = path.read_bytes()
        for name in ('assembly.py', 'assembly_step.py', 'limits.py', 'capabilities.json'):
            self.contents['inventor_kit/' + name] = b'{}'
        for path in [ROOT / 'LICENSE', ROOT / 'THIRD_PARTY_NOTICES.md', *sorted((ROOT / 'licenses').glob('*.txt'))]:
            self.contents[info + '/licenses/' + path.relative_to(ROOT).as_posix()] = path.read_bytes()

    def rows(self, separator):
        rows = []
        for name, data in self.contents.items():
            digest = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).decode().rstrip('=')
            rows.append([name.replace('/', separator), 'sha256=' + digest, str(len(data))])
        rows.append([self.record.replace('/', separator), '', ''])
        return rows

    def validate(self, rows, extra_files=None):
        record = io.StringIO(newline='')
        csv.writer(record).writerows(rows)
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / f'inventor_kit-{self.version}-cp310-abi3-win_amd64.whl'
            with zipfile.ZipFile(path, 'w') as archive:
                for name, data in (self.contents | (extra_files or {})).items():
                    archive.writestr(name, data)
                archive.writestr(self.record, record.getvalue().encode())
            return check(path)

    def test_windows_record_separators_are_host_independent(self):
        for separator in ('/', '\\'):
            with self.subTest(separator=separator):
                self.assertTrue(self.validate(self.rows(separator))['registry_only_required'])

    def test_mixed_separators_are_accepted(self):
        rows = self.rows('\\')
        for row in rows[::2]:
            row[0] = row[0].replace('\\', '/')
        self.validate(rows)

    def test_duplicate_record_aliases_are_rejected(self):
        rows = self.rows('\\')
        rows.append([rows[0][0].replace('\\', '/'), *rows[0][1:]])
        with self.assertRaisesRegex(ValueError, 'Duplicate RECORD member'):
            self.validate(rows)

    def test_windows_records_still_verify_hashes_and_sizes(self):
        for field, value in ((1, 'sha256=wrong'), (2, '0')):
            with self.subTest(field=field):
                rows = self.rows('\\')
                rows[0][field] = value
                with self.assertRaisesRegex(ValueError, 'RECORD digest/size mismatch'):
                    self.validate(rows)

    def test_missing_and_unrecorded_files_are_rejected(self):
        rows = self.rows('\\')
        rows[0][0] = 'inventor_kit\\missing.pyd'
        with self.assertRaisesRegex(ValueError, 'Missing RECORD member'):
            self.validate(rows)
        with self.assertRaisesRegex(ValueError, 'Unrecorded wheel members'):
            self.validate(self.rows('\\')[:-1])
        with self.assertRaisesRegex(ValueError, 'Unrecorded wheel members'):
            self.validate(self.rows('\\'), {'inventor_kit/unlisted.py': b'content'})

    def test_parent_traversal_is_not_normalized_away(self):
        rows = self.rows('\\')
        rows[0][0] = 'inventor_kit\\..\\' + rows[0][0]
        with self.assertRaisesRegex(ValueError, 'Missing RECORD member'):
            self.validate(rows)

    def test_viewer_assets_are_checked_beyond_record_integrity(self):
        import json
        prefix = 'inventor_kit/viewer/static/'
        manifest = json.loads(self.contents[prefix+'manifest.json'])
        js = next(name for name in manifest['outputs'] if name.endswith('.js'))
        self.contents[prefix+js] = b'wrong build'
        with self.assertRaisesRegex(ValueError, 'stale viewer asset'):
            self.validate(self.rows('/'))


if __name__ == '__main__':
    unittest.main()
