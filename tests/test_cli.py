"""Actual subprocess entrypoints, partial export, and failed batch isolation."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

from inventor_kit.cli import run_job

ROOT = Path(__file__).resolve().parents[1]
CORPUS = ROOT / 'fixtures/public'


class CLI(unittest.TestCase):
    def cli(self, *args):
        return subprocess.run([sys.executable, '-m', 'inventor_kit', *map(str, args)],
                              capture_output=True, text=True, timeout=90)

    def test_legacy_inspection_and_metadata_jsonl(self):
        result = self.cli(CORPUS / 'SamplePart.ipt')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)['status'], 'decoded_subset')
        result = self.cli(CORPUS / 'SamplePart.ipt', '--metadata-only', '--jsonl')
        report = json.loads(result.stdout)
        self.assertEqual(report['stages']['conversion'], 'not_attempted')
        self.assertEqual(report['document']['document']['stages']['geometry'], 'not_attempted')

    def test_body_selection_export_keeps_omissions(self):
        path = CORPUS / 'INV_nist_ftc_06_asme1_2021.ipt'
        result = self.cli(path, '--list-bodies')
        self.assertEqual(result.returncode, 2, result.stderr)
        inventory = json.loads(result.stdout)
        body_id = inventory['bodies'][0]['id']
        with tempfile.TemporaryDirectory() as temporary:
            step = Path(temporary) / 'selected.step'
            args = (path, '--body-id', body_id, '--step', step, '--jsonl')
            refused = self.cli(*args)
            self.assertEqual(refused.returncode, 1, refused.stderr)
            self.assertFalse(step.exists())
            accepted = self.cli(*args, '--allow-partial')
            self.assertEqual(accepted.returncode, 2, accepted.stderr)
            report = json.loads(accepted.stdout)
            self.assertTrue(report['selection_complete'])
            self.assertFalse(report['geometry_complete'])
            self.assertEqual(report['export']['path'], str(step))
            sidecar = json.loads(step.with_suffix('.step.json').read_text())
            self.assertEqual(len(sidecar['omissions']), 2)
            original = step.read_bytes()
            self.assertEqual(self.cli(*args, '--allow-partial').returncode, 1)
            self.assertEqual(step.read_bytes(), original)

    def test_failed_batch_continues_and_jsonl_has_no_native_log_text(self):
        with tempfile.TemporaryDirectory() as temporary:
            bad = Path(temporary) / 'bad.ipt'
            bad.write_bytes(b'not CFB')
            report_path = Path(temporary) / 'jobs.jsonl'
            result = self.cli(bad, CORPUS / 'SamplePart.ipt', '--output-dir', Path(temporary) / 'steps',
                              '--jsonl', '--report', report_path)
            self.assertEqual(result.returncode, 1)
            reports = [json.loads(line) for line in result.stdout.splitlines()]
            self.assertEqual([r['status'] for r in reports], ['error', 'success'])
            self.assertEqual(result.stdout, report_path.read_text())
            self.assertTrue((Path(temporary) / 'steps/SamplePart.step').exists())
            self.assertFalse((Path(temporary) / 'steps/bad.step').exists())

    def test_iam_and_unsupported_drawing(self):
        path = CORPUS / 'm5-samplebg/Subassembly.iam'
        self.assertEqual(self.cli(path, '--convert').returncode, 1)
        result = self.cli(path, '--convert', '--allow-unverified-state')
        report = json.loads(result.stdout)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(report['converted_instances'], 1)
        self.assertFalse(report['current_state_verified'])
        result = self.cli(CORPUS / 'SampleBg.idw', '--convert')
        self.assertEqual(result.returncode, 3)
        report = json.loads(result.stdout)
        self.assertEqual(report['kind'], 'drawing')
        self.assertEqual(report['units'], 'source_units_unverified')
        import jsonschema
        jsonschema.validate(report, json.loads((ROOT/'schemas/conversion-report-v1.schema.json').read_text()))

    def test_timeout_and_crash_cannot_publish_staged_step(self):
        with tempfile.TemporaryDirectory() as temporary:
            target = Path(temporary) / 'interrupted.step'
            request = dict(path=str(CORPUS / 'SamplePart.ipt'), step=str(target))

            def crash(command, **kwargs):
                payload = json.loads(kwargs['input'])
                Path(payload['step']).write_bytes(b'partial native write')
                return subprocess.CompletedProcess(command, -11)

            with patch('inventor_kit.cli.subprocess.run', side_effect=crash):
                report = run_job(request, 10)
            self.assertEqual(report['diagnostics'][0]['code'], 'execution.worker_failed')
            self.assertFalse(target.exists())
            result = self.cli(CORPUS / 'SamplePart.ipt', '--step', target, '--timeout', '0.001')
            self.assertEqual(result.returncode, 1)
            self.assertEqual(json.loads(result.stdout)['diagnostics'][0]['code'], 'execution.timeout')
            self.assertFalse(target.exists())

    def test_batch_name_collision_and_invalid_options_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            target = Path(temporary) / 'steps'
            result = self.cli(CORPUS / 'SamplePart.ipt', CORPUS / 'SamplePart.ipt', '--output-dir', target)
            self.assertEqual(result.returncode, 1)
            self.assertFalse(target.exists())
            target.write_text('existing file')
            result = self.cli(CORPUS / 'SamplePart.ipt', '--output-dir', target)
            self.assertEqual(result.returncode, 1)
            self.assertIn('Could not create output directory', result.stderr)
            self.assertNotIn('Traceback', result.stderr)
            self.assertEqual(target.read_text(), 'existing file')
        for args in (('--metadata-only', '--step', 'never.step'), ('--timeout', 'nan')):
            self.assertEqual(self.cli(CORPUS / 'SamplePart.ipt', *args).returncode, 1)
