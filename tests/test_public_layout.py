"""Public material must remain usable without private files or diagnostic payloads."""
import json
from pathlib import Path
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
from check_public_tree import check_links
from summarize_validation import summarize


class PublicLayout(unittest.TestCase):
    def test_public_documentation_links_cannot_require_internal_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / 'docs').mkdir()
            (root / 'docs/api.md').write_text('API')
            readme = root / 'README.md'
            readme.write_text('[API](docs/api.md)')
            self.assertEqual(check_links(root), 2)
            for target in ('internal/notes.md', 'docs/missing.md', '../outside.md'):
                readme.write_text(f'[details]({target})')
                with self.assertRaises(ValueError):
                    check_links(root)

    def test_summary_excludes_private_fields_and_requires_complete_inputs(self):
        private = 'PRIVATE_DIAGNOSTIC_SENTINEL'
        counts = {'total': 2, 'decoded_subset': 1, 'converted': 1}
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            reports = {
                'python_tests.json': dict(status='passed', tests=1, skipped=0, failures=0, errors=0,
                                          fixtures=4, holdouts=2, log=private),
                'public_validation.json': dict(failures=[], by_split={s: counts for s in ('regression', 'holdout')},
                                               results=[dict(source_path=private, raw_data=private)]),
                'geometry_validation.json': dict(splits={s: dict(converted=1) for s in ('regression', 'holdout')},
                                                 module_files={'native': private}),
                'assembly_validation.json': dict(regression_passed=True, fixtures=2,
                    holdout=dict(status='unavailable', diagnostics=[private]), source_documents=[private]),
                'fuzz.json': dict(status='passed', holdouts_used=False, logs=[private]),
            }
            for name, value in reports.items():
                (root / name).write_text(json.dumps(value))
            result = summarize(root, recorded_on='2026-09-10', environment='local')
            self.assertEqual(result['status'], 'passed')
            self.assertNotIn(private, json.dumps(result))
            self.assertFalse(result['assembly']['current_state_verified'])
            reports['python_tests.json']['skipped'] = 1
            (root / 'python_tests.json').write_text(json.dumps(reports['python_tests.json']))
            self.assertEqual(summarize(root, recorded_on='2026-09-10', environment='local')['status'], 'failed')
            (root / 'fuzz.json').unlink()
            with self.assertRaises(FileNotFoundError):
                summarize(root, recorded_on='2026-09-10', environment='local')

    def test_public_baselines_keep_source_identity_and_holdout_separation(self):
        manifest = {row['file']: row for name in ('manifest.json', 'assembly-manifest.json')
                    for row in json.loads((ROOT / 'fixtures' / name).read_text())}
        baseline = json.loads((ROOT / 'tests/data/geometry-baseline.json').read_text())
        cases = json.loads((ROOT / 'benchmarks/cases.json').read_text())
        self.assertTrue(baseline['results'])
        self.assertTrue(cases['cases'])
        for row in baseline['results'] + cases['cases']:
            source = manifest[row['file']]
            self.assertEqual(source['split'], 'regression')
            self.assertEqual(row['sha256'], source['sha256'])
        self.assertNotIn('reference_environment', cases)


if __name__ == '__main__':
    unittest.main()
