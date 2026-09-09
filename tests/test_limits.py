"""Public budgets, immutable input sharing and limits passed through assembly conversion."""
from concurrent.futures import ThreadPoolExecutor
from dataclasses import asdict
import json
from pathlib import Path
import unittest

import inventor_kit as ik

ROOT = Path(__file__).resolve().parents[1]
PUBLIC = ROOT / 'fixtures/public'


class LimitsAPI(unittest.TestCase):
    def test_defaults_and_native_hard_caps_agree(self):
        self.assertEqual(asdict(ik.Limits()), json.loads(ik._inventor.default_limits()))
        for key, cap in asdict(ik.Limits()).items():
            for value in (-1, cap + 1, True, 1.5, '1'):
                with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                    ik.Limits(**{key: value})
            with self.assertRaises(ValueError):
                ik._inventor.inspect(b'', 'test', False, json.dumps({key: cap + 1}))
        for value in ('{"unknown": 1}', '{"max_file_bytes": true}', '{"max_file_bytes": -1}'):
            with self.assertRaises(ValueError):
                ik._inventor.inspect(b'', 'test', False, value)
        with self.assertRaises(TypeError):
            ik.inspect(b'', limits={})

    def test_file_limits_are_applied_before_open_and_through_native_entrypoints(self):
        part = PUBLIC / 'SamplePart.ipt'
        if not part.exists():
            self.skipTest('Run scripts/fetch_public_samples.py')
        data = part.read_bytes()
        tiny = ik.Limits(max_file_bytes=len(data) - 1)
        for call in (lambda: ik.read(data, limits=tiny), lambda: ik.inspect(data, limits=tiny),
                     lambda: ik.inspect_assembly(data, limits=tiny), lambda: ik.read_file(part, limits=tiny),
                     lambda: ik.inspect_file(part, limits=tiny), lambda: ik.inspect_assembly_file(part, limits=tiny)):
            with self.assertRaisesRegex(ValueError, 'file byte limit'):
                call()
        metadata = ik.inspect(data, limits=ik.Limits(max_inflated_bytes=0))
        self.assertEqual(metadata.metadata.stages.geometry, 'not_attempted')
        decoded = ik.read(data, limits=ik.Limits(max_total_inflated_bytes=0))
        self.assertIsNone(decoded.model)
        self.assertTrue(decoded.summary['diagnostics'])

    def test_shared_immutable_buffer_and_assembly_limits(self):
        path = PUBLIC / 'm5-samplebg/Subassembly.iam'
        if not path.exists():
            self.skipTest('Run scripts/fetch_assembly_samples.py')
        data = path.read_bytes()
        with ThreadPoolExecutor(max_workers=4) as pool:
            results = list(pool.map(lambda _: ik.inspect_assembly(data), range(12)))
        self.assertTrue(all(r.summary['source_sha256'] == results[0].summary['source_sha256'] for r in results))
        for mutable in (bytearray(data), memoryview(data)):
            with self.assertRaises(TypeError):
                ik.inspect(mutable)
        with self.assertRaisesRegex(ValueError, 'file byte limit'):
            ik.read_assembly_file(path, limits=ik.Limits(max_file_bytes=1))
        # Metadata/resolution can succeed with B-rep inflation disabled; conversion
        # must honor the same setting rather than silently using default limits.
        graph = ik.read_assembly_file(path, limits=ik.Limits(max_candidates=0))
        self.assertEqual(graph.structure_status, 'resolved')
        result = graph.to_cadquery(allow_unverified_state=True, allow_partial=True)
        self.assertEqual(result.converted_instances, 0)
        self.assertEqual(result.omissions[0].reason, 'geometry_unavailable')


if __name__ == '__main__':
    unittest.main()
