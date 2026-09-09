from copy import deepcopy
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'scripts'))
from corpus_manifest import load_manifest, validate_manifest
from oracle_contract import compare_capture, validate_capture
from capture_vendor_oracle import collect_geometry
from check_dependencies import check_graph


def capture():
    return {
        'schema_version': 1,
        'source': {'sha256': 'a'*64, 'bytes': 512, 'file_name': 'synthetic.ipt'},
        'provider': {'name': 'synthetic', 'version': 'test-1', 'platform': 'test', 'collector_sha256': 'b'*64},
        'capture': {'acquired_at': '2026-09-09T00:00:00Z', 'scope': 'synthetic', 'model_state': None, 'source_modified': False, 'update_performed': False},
        'document': {'kind': 'part', 'internal_name': None, 'properties': [], 'references': [], 'diagnostics': []},
        'geometry': {'status': 'available', 'units': 'mm', 'body_count': 1, 'solid_count': 1, 'faces': 6, 'volume_mm3': 6.0, 'area_mm2': 22.0, 'bbox_mm': [0,0,0,1,2,3], 'bbox_kind': 'precise'},
        'limitations': ['Synthetic test data, not an Autodesk capture.'],
    }


class CorpusSplits(unittest.TestCase):
    def test_current_split_preserves_regressions_and_reserves_new_family(self):
        regression, holdout = load_manifest(split='regression'), load_manifest(split='holdout')
        self.assertEqual(len(regression), 30)
        self.assertEqual(len(holdout), 3)
        self.assertTrue({r['family_id'] for r in regression}.isdisjoint(r['family_id'] for r in holdout))

    def test_family_or_identical_bytes_cannot_cross_splits(self):
        rows = [deepcopy(load_manifest(split='regression')[0]), deepcopy(load_manifest(split='holdout')[0])]
        for field in ('family_id', 'sha256'):
            bad = deepcopy(rows)
            bad[1][field] = bad[0][field]
            with self.subTest(field=field), self.assertRaises(ValueError):
                validate_manifest(bad)

    def test_duplicate_names_and_unsafe_paths_are_rejected(self):
        row = deepcopy(load_manifest()[0])
        for name in ('../part.ipt', 'dir\\part.ipt', '..'):
            row['file'] = name
            with self.subTest(name=name), self.assertRaises(ValueError):
                validate_manifest([row])
        with self.assertRaises(ValueError):
            validate_manifest([load_manifest()[0], load_manifest()[0]])


class DependencyIdentity(unittest.TestCase):
    def graph(self):
        registry = 'registry+https://github.com/rust-lang/crates.io-index'
        return {
            'packages': [dict(id=i, name=name, version='0.2.1', source=source, manifest_path='/fixture/'+i+'/Cargo.toml')
                         for i,name,source in [('root','inventor-py',None),('core','acis-core',registry),('bridge','acis-py-bridge',registry)]],
            'resolve': {'nodes': [dict(id='root', dependencies=['core','bridge']),dict(id='core',dependencies=[]),dict(id='bridge',dependencies=['core'])]},
        }

    def test_registry_graph_and_explicit_development_override(self):
        graph = self.graph()
        self.assertEqual(check_graph(graph)['acis-core']['version'], '0.2.1')
        graph['packages'][2]['source'] = None
        with self.assertRaises(ValueError):
            check_graph(graph)
        check_graph(graph, allow_local_bridge=True)
        graph['packages'][1]['source'] = None
        with self.assertRaises(ValueError):
            check_graph(graph, allow_local_bridge=True)

    def test_second_core_identity_is_rejected_only_when_linked(self):
        graph = self.graph()
        graph['packages'].append(dict(id='other-core', name='acis-core', version='0.2.1', source=None, manifest_path='/other/Cargo.toml'))
        graph['resolve']['nodes'].append(dict(id='other-core', dependencies=[]))
        check_graph(graph)  # An unrelated workspace member is not linked here.
        graph['resolve']['nodes'][2]['dependencies'].append('other-core')
        with self.assertRaises(ValueError):
            check_graph(graph, allow_local_bridge=True)


class OracleContract(unittest.TestCase):
    def test_synthetic_match_never_proves_native_state(self):
        data = capture()
        result = compare_capture(data, data['geometry'])
        self.assertEqual(result['status'], 'matched_metrics')
        self.assertEqual(result['provider']['name'], 'synthetic')
        self.assertEqual(result['state_verification'], 'unverified')

    def test_wrong_source_and_nonfinite_metrics_are_rejected(self):
        data = capture()
        with self.assertRaises(ValueError):
            validate_capture(data, source_sha256='c'*64)
        data['geometry']['volume_mm3'] = float('inf')
        with self.assertRaises(ValueError):
            validate_capture(data)

    def test_bad_schema_and_mutated_capture_are_rejected(self):
        from jsonschema import ValidationError
        for key, value in [('schema_version', 2), ('capture', {**capture()['capture'], 'source_modified': True})]:
            data = capture()
            data[key] = value
            with self.subTest(key=key), self.assertRaises(ValidationError):
                validate_capture(data)

    def test_mismatch_and_missing_geometry_are_distinct(self):
        data = capture()
        actual = {**data['geometry'], 'volume_mm3': 60.0}
        self.assertEqual(compare_capture(data, actual)['status'], 'mismatch')
        self.assertEqual(compare_capture(data, None)['status'], 'not_compared')
        data['geometry'] = {'status': 'unavailable', 'units': 'mm', 'reason': 'API unavailable'}
        self.assertEqual(compare_capture(data, actual)['status'], 'not_compared')

    def test_enclosing_bbox_is_not_used_as_exact_bounds(self):
        data = capture()
        data['geometry']['bbox_kind'] = 'enclosing'
        result = compare_capture(data, {**data['geometry'], 'bbox_mm': [-1,-1,-1,4,4,4]})
        self.assertNotIn('bbox_mm', result['checks'])
        self.assertTrue(any('not marked precise' in s for s in result['limitations']))

    def test_apprentice_cannot_claim_active_state_capture(self):
        from jsonschema import ValidationError
        data = capture()
        data['provider']['name'] = 'autodesk.apprentice'
        data['capture']['scope'] = 'active_model_state'
        with self.assertRaises(ValidationError):
            validate_capture(data)

    def test_missing_com_geometry_is_visible_without_invented_values(self):
        result = collect_geometry(object())
        self.assertEqual(result['status'], 'unavailable')
        self.assertNotIn('volume_mm3', result)


class BridgeContract(unittest.TestCase):
    def test_native_bridge_rejects_incompatible_python_layout(self):
        from unittest.mock import patch
        import inventor_kit
        import cq_acis.model
        path = Path(__file__).resolve().parents[1] / 'fixtures/public/Cylinder.ipt'
        if not path.exists():
            self.fail('Required regression corpus is missing; fetch_public_samples.py must run first')
        with patch.object(cq_acis.model, 'MODEL_API_VERSION', 999):
            with self.assertRaisesRegex(ImportError, 'model API 2'):
                inventor_kit.read_file(path)
            # Bypass the public Python guard to exercise the Rust bridge itself.
            with self.assertRaisesRegex(ImportError, 'model API 2'):
                inventor_kit._inventor.read(path.read_bytes(), 'wrong-layout')


if __name__ == '__main__':
    unittest.main()

class DevelopmentCoreDependencyTests(unittest.TestCase):
    def test_local_core_needs_its_own_explicit_opt_in(self):
        registry = 'registry+https://github.com/rust-lang/crates.io-index'
        graph = {
            'packages': [dict(id=i,name=name,version='0.2.1',source=source,manifest_path='/fixture/'+i+'/Cargo.toml')
                         for i,name,source in [('root','inventor-py',None),('core','acis-core',None),('bridge','acis-py-bridge',registry)]],
            'resolve': {'nodes': [dict(id='root',dependencies=['core','bridge']),dict(id='core',dependencies=[]),dict(id='bridge',dependencies=['core'])]},
        }
        with self.assertRaises(ValueError):check_graph(graph,allow_local_bridge=True)
        self.assertEqual(check_graph(graph,allow_local_core=True)['acis-core']['version'],'0.2.1')
