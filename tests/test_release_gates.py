"""Reject missing corpus, incomplete platform sets and conflicting publish retries."""
from copy import deepcopy
import hashlib
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
import urllib.error

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'scripts'))
from check_corpus import verify
from check_release import artifact_set, check_tag, pypi_conflicts
import inventor_kit as ik


class ReleaseGates(unittest.TestCase):
    def test_corpus_missing_changed_and_escaped_are_errors(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            part = root / 'sample.ipt'
            row = dict(file=part.name, bytes=3, sha256=hashlib.sha256(b'abc').hexdigest(), split='holdout')
            manifest = root / 'manifest.json'
            manifest.write_text(json.dumps([row]))
            with self.assertRaisesRegex(ValueError, 'Missing'):
                verify(root, [manifest])
            part.write_bytes(b'abc')
            self.assertEqual(verify(root, [manifest])['holdouts'], 1)
            part.write_bytes(b'abd')
            with self.assertRaisesRegex(ValueError, 'hash mismatch'):
                verify(root, [manifest])
            manifest.write_text(json.dumps([{**row, 'file': '../sample.ipt'}]))
            with self.assertRaisesRegex(ValueError, 'Unsafe'):
                verify(root, [manifest])

    def test_release_requires_exact_tag_and_each_platform_once(self):
        version = check_tag('')
        self.assertEqual(check_tag('v' + version), version)
        with self.assertRaises(ValueError):
            check_tag('v9.9.9')
        paths = [Path('inventor_kit-0.1.0-cp310-abi3-' + s + '.whl') for s in
                 ('manylinux_2_17_x86_64.manylinux2014_x86_64', 'win_amd64', 'macosx_11_0_arm64', 'macosx_10_12_x86_64')]
        paths.append(Path('inventor_kit-0.1.0.tar.gz'))
        with patch('check_release.check', side_effect=lambda p: {'file': str(p)}):
            self.assertEqual(len(artifact_set(paths)), 5)
            for bad in (paths[:-1], paths + paths[:1], paths + [Path('unknown.txt')]):
                with self.assertRaises(ValueError):
                    artifact_set(bad)

    def test_pypi_retry_requires_identical_hashes_and_network_errors_fail(self):
        reports = [dict(file='package.whl', sha256='a'*64)]
        same = dict(urls=[dict(filename='package.whl', digests=dict(sha256='a'*64))])
        with patch('urllib.request.urlopen', return_value=io.BytesIO(json.dumps(same).encode())):
            pypi_conflicts('0.1.0', reports)
        changed = deepcopy(same)
        changed['urls'][0]['digests']['sha256'] = 'b'*64
        with patch('urllib.request.urlopen', return_value=io.BytesIO(json.dumps(changed).encode())):
            with self.assertRaises(ValueError):
                pypi_conflicts('0.1.0', reports)
        for status in (404, 500):
            with patch('urllib.request.urlopen', side_effect=urllib.error.HTTPError('url', status, 'test', None, None)):
                if status == 404:
                    pypi_conflicts('0.1.0', reports)
                else:
                    with self.assertRaises(urllib.error.HTTPError):
                        pypi_conflicts('0.1.0', reports)

    def test_capabilities_do_not_promote_saved_geometry_to_current_state(self):
        caps = ik.capabilities()
        self.assertEqual(caps['support_level'], 'verified_subset')
        self.assertFalse(caps['part_geometry']['current_state_verified'])
        self.assertFalse(caps['assembly']['current_state_verified'])
        caps['assembly']['section_versions'].clear()
        self.assertEqual(len(ik.capabilities()['assembly']['section_versions']), 27)


if __name__ == '__main__':
    unittest.main()
