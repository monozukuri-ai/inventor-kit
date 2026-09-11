"""Run viewer integration tests with required dependencies and fixtures; no skips."""
import argparse
import importlib.util
import json
from pathlib import Path
import unittest

from check_corpus import ROOT, verify


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT/'internal/reports/latest/viewer_tests.json')
    args = parser.parse_args()
    for package in ('ocp_tessellate', 'jsonschema', 'cadquery'):
        if importlib.util.find_spec(package) is None:
            raise RuntimeError(f'Required viewer test dependency is absent: {package}')
    verify(ROOT/'fixtures/public')
    suite = unittest.defaultTestLoader.discover(str(ROOT/'viewer_tests'))
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    passed = result.wasSuccessful() and not result.skipped and result.testsRun > 0
    report = dict(status='passed' if passed else 'failed', tests=result.testsRun,
                  failures=len(result.failures), errors=len(result.errors), skipped=len(result.skipped),
                  scope='Local scene, mesh, worker and server tests; browser and platform checks are separate')
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2)+'\n')
    raise SystemExit(0 if passed else 1)


if __name__ == '__main__':
    main()
