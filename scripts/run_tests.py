"""Corpus validation and unittest runner that treats every skip as a CI failure."""
import argparse
import json
from pathlib import Path
import unittest

from check_corpus import ROOT, verify


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT / 'internal/reports/latest/python_tests.json')
    args = parser.parse_args()
    corpus = verify(ROOT / 'fixtures/public')
    suite = unittest.defaultTestLoader.discover(str(ROOT / 'tests'))
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    passed = result.wasSuccessful() and not result.skipped and result.testsRun > 0
    report = dict(status='passed' if passed else 'failed', tests=result.testsRun,
                  skipped=len(result.skipped), failures=len(result.failures), errors=len(result.errors),
                  fixtures=corpus['fixtures'], holdouts=corpus['holdouts'])
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    raise SystemExit(0 if passed else 1)


if __name__ == '__main__':
    main()
