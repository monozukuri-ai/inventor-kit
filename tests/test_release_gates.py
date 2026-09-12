"""Reject missing corpus, incomplete platform sets and conflicting publish retries."""
from copy import deepcopy
from email.message import Message
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
from check_release import artifact_set, check_tag, pypi_conflicts, viewer_reports
from check_distribution import check_metadata
import inventor_kit as ik


class ReleaseGates(unittest.TestCase):
    def test_distribution_rejects_stale_python_and_cq_acis_requirements(self):
        meta = Message()
        for name, value in [('Name', 'inventor-kit'), ('Version', '0.1.0'),
                            ('License-Expression', 'MIT'), ('Requires-Python', '>=3.11'),
                            ('Requires-Dist', 'cq-acis<0.4,>=0.3.3'), ('Provides-Extra', 'viewer'),
                            ('Requires-Dist', 'ocp-tessellate==3.5.1; extra == "viewer"')]:
            meta[name] = value
        check_metadata(meta, '0.1.0')
        for name, value in [('Requires-Python', '>=3.10'),
                            ('Requires-Dist', 'cq-acis>=0.3.2,<0.4')]:
            bad = deepcopy(meta)
            bad.replace_header(name, value)
            with self.subTest(name=name), self.assertRaises(ValueError):
                check_metadata(bad, '0.1.0')

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

    def test_viewer_requires_nine_successful_installs_bound_to_archive_hashes(self):
        names = ['inventor_kit-0.1.0-cp310-abi3-' + tag + '.whl' for tag in
                 ('manylinux_2_17_x86_64.manylinux2014_x86_64', 'win_amd64', 'macosx_11_0_arm64', 'macosx_10_12_x86_64')]
        names.append('inventor_kit-0.1.0.tar.gz')
        artifacts = [dict(file=name, sha256=str(i)*64) for i, name in enumerate(names)]
        platforms = ['linux-x86_64', 'windows-x86_64', 'macos-arm64', 'macos-x86_64', 'linux-x86_64']
        reports = []
        for index, (artifact, platform) in enumerate(zip(artifacts, platforms)):
            for python in (('3.11.13',) if index == 4 else ('3.11.13', '3.12.9')):
                reports.append(dict(schema_version=1, artifact=artifact, platform=platform, python=python,
                    status='passed', dependency_mode='published', pip_check='passed',
                    interpreter_shutdown='passed', current_state_verified=False,
                    dependencies={'ocp-tessellate': '3.5.1'}, cases={
                        name: dict(displayed_instances=parts, occurrences=occurrences, omissions=omissions,
                                   mesh_buffers_fetched=4, shutdown='passed')
                        for name, parts, occurrences, omissions in [('part', 1, 1, 0), ('assembly', 1, 1, 0), ('partial_assembly', 5, 7, 2)]}))
        with tempfile.TemporaryDirectory() as temporary:
            paths = [Path(temporary) / f'{i}.json' for i in range(len(reports))]
            for path, report in zip(paths, reports):
                path.write_text(json.dumps(report))
            self.assertEqual(viewer_reports(paths, artifacts), 9)
            for bad in (paths[:-1], paths + paths[:1]):
                with self.assertRaises(ValueError):
                    viewer_reports(bad, artifacts)
            mutations = [
                {'artifact': {**reports[0]['artifact'], 'sha256': 'bad'}},
                {'platform': 'windows-x86_64'}, {'python': '3.13.0'},
                {'status': 'failed'}, {'dependency_mode': 'local'}, {'pip_check': 'failed'},
                {'interpreter_shutdown': 'failed'}, {'current_state_verified': True},
                {'dependencies': {'ocp-tessellate': '0.0.0'}}, {'cases': {}},
                {'cases': {**reports[0]['cases'], 'partial_assembly': {
                    **reports[0]['cases']['partial_assembly'], 'displayed_instances': 0}}},
                {'cases': {**reports[0]['cases'], 'part': {
                    **reports[0]['cases']['part'], 'shutdown': 'failed'}}},
            ]
            for mutation in mutations:
                paths[0].write_text(json.dumps({**reports[0], **mutation}))
                with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                    viewer_reports(paths, artifacts)

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
