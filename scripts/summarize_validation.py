"""Publish selected public-corpus counts; keep paths, diagnostics and raw logs internal."""
import argparse
from datetime import date
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def summarize(directory, *, recorded_on, environment):
    def read(name):
        return json.loads((directory / name).read_text())

    tests = read('python_tests.json')
    parts = read('public_validation.json')
    geometry = read('geometry_validation.json')
    assembly = read('assembly_validation.json')
    fuzz = read('fuzz.json')
    # Select fields explicitly. New diagnostics, source paths or captures in the
    # detailed reports must never become public just by extending those reports.
    passed = (tests['status'] == 'passed' and tests['skipped'] == 0
              and not parts['failures'] and assembly['regression_passed']
              and fuzz['status'] == 'passed')
    for split in ('regression', 'holdout'):
        if geometry['splits'][split]['converted'] != parts['by_split'][split]['converted']:
            raise ValueError('Public and geometry validation counts disagree')
    return {
        'schema_version': 1,
        'recorded_on': date.fromisoformat(recorded_on).isoformat(),
        'environment': environment,
        'status': 'passed' if passed else 'failed',
        'scope': 'Fixed public corpus regression; not vendor/current-state or platform release qualification',
        'python_tests': {k: tests[k] for k in ('tests', 'skipped', 'failures', 'errors')},
        'corpus': {k: tests[k] for k in ('fixtures', 'holdouts')},
        'parts': {
            split: {k: parts['by_split'][split][k] for k in ('total', 'decoded_subset', 'converted')}
            for split in ('regression', 'holdout')
        },
        'assembly': {
            'fixture_entries': assembly['fixtures'],
            'regression_passed': assembly['regression_passed'],
            'unsupported_profile_holdout_retained': assembly['holdout']['status'] == 'unavailable',
            'current_state_verified': False,
        },
        'fuzz': {'passed': fuzz['status'] == 'passed', 'holdouts_used': fuzz['holdouts_used'],
                 'scope': 'Time-bounded ASan mutation smoke; not exhaustive safety proof'},
        'release_qualification': 'Reported separately by the platform build/install jobs',
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input', type=Path, default=ROOT / 'internal/reports/latest')
    parser.add_argument('--output', type=Path, default=ROOT / 'reports/validation-summary.json')
    parser.add_argument('--date', default=date.today().isoformat())
    parser.add_argument('--environment', choices=('local', 'github-actions'), default='local')
    args = parser.parse_args()
    report = summarize(args.input, recorded_on=args.date, environment=args.environment)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({'summary': str(args.output), 'status': report['status']}))
    raise SystemExit(0 if report['status'] == 'passed' else 1)


if __name__ == '__main__':
    main()
