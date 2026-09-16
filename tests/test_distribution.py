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
from check_license import DISTRIBUTION_LICENSE, PINNED_TEXTS, check_pinned_texts, license_paths


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
                f'License-Expression: {DISTRIBUTION_LICENSE}\nRequires-Python: >=3.11\n'
                'Requires-Dist: cq-acis>=0.3.8,<0.4\n\n'
            ).encode(),
            info + '/WHEEL': b'Wheel-Version: 1.0\nRoot-Is-Purelib: false\nTag: cp310-abi3-win_amd64\n',
        }
        self.contents[info+'/METADATA'] = self.contents[info+'/METADATA'].rstrip() + b'\nProvides-Extra: viewer\nRequires-Dist: ocp-tessellate==3.5.1; extra == "viewer"\n\n'
        for path in (ROOT/'python/inventor_kit/viewer').rglob('*'):
            if path.is_file() and (path.suffix == '.py' or 'static' in path.parts):
                self.contents['inventor_kit/viewer/'+path.relative_to(ROOT/'python/inventor_kit/viewer').as_posix()] = path.read_bytes()
        for name in ('assembly.py', 'assembly_step.py', 'conversion.py', 'cli.py', '_cli_worker.py', 'limits.py', 'capabilities.json'):
            self.contents['inventor_kit/' + name] = b'{}'
        for name in license_paths():
            self.contents[info + '/licenses/' + name] = (ROOT/name).read_bytes()
        self.metadata = info + '/METADATA'
        self.license_prefix = info + '/licenses/'
        self.contents[self.metadata] = self.contents[self.metadata].rstrip() + b'\n' + (
            ''.join('License-File: ' + p + '\n' for p in license_paths()) + '\n').encode()

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


    def test_mit_only_metadata_is_rejected_even_with_valid_record(self):
        self.contents[self.metadata] = self.contents[self.metadata].replace(
            DISTRIBUTION_LICENSE.encode(), b'MIT')
        with self.assertRaisesRegex(ValueError, 'license'):
            self.validate(self.rows('/'))

    def test_license_must_be_in_the_declared_dist_info_location(self):
        path = self.license_prefix + 'licenses/PolyForm-Noncommercial-1.0.0.md'
        self.contents['decoy/licenses/PolyForm-Noncommercial-1.0.0.md'] = self.contents.pop(path)
        with self.assertRaisesRegex(ValueError, 'Missing license/notice'):
            self.validate(self.rows('/'))

    def test_stale_notices_are_rejected_even_with_valid_record(self):
        for name in ('LICENSE', 'COMMERCIAL-LICENSE.md', 'licenses/inventor-kit-legacy-MIT.txt'):
            key = self.license_prefix + name
            original = self.contents[key]
            self.contents[key] = b'old or incomplete legal material'
            with self.subTest(name=name), self.assertRaisesRegex(ValueError, 'Stale license/notice'):
                self.validate(self.rows('/'))
            self.contents[key] = original

    def test_missing_and_duplicate_license_declarations_are_rejected(self):
        original = self.contents[self.metadata]
        line = b'License-File: LICENSE\n'
        for value in (original.replace(line, b''), original.replace(line, line+line)):
            self.contents[self.metadata] = value
            with self.assertRaisesRegex(ValueError, 'License-File'):
                self.validate(self.rows('/'))

    def test_upstream_and_legacy_texts_are_independently_pinned(self):
        for changed in PINNED_TEXTS:
            def read(name):
                value = (ROOT/name).read_bytes()
                return value + b'changed' if name == changed else value
            with self.subTest(changed=changed), self.assertRaisesRegex(ValueError, 'Changed canonical'):
                check_pinned_texts(read)

    def test_viewer_notice_cannot_be_removed_from_a_valid_record(self):
        del self.contents['inventor_kit/viewer/static/LICENSE.txt']
        with self.assertRaisesRegex(ValueError, 'Missing viewer Required Notice'):
            self.validate(self.rows('/'))


if __name__ == '__main__':
    unittest.main()
