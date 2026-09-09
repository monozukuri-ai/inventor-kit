"""Fixed-corpus timings and process peak RSS; timings are host-specific evidence."""
import argparse
import hashlib
import importlib.metadata
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
CASE_ROWS = json.loads((ROOT / 'benchmarks/cases.json').read_text())['cases']
CASES = [(row['operation'], row['file']) for row in CASE_ROWS]


def measure(operation, path, iterations):
    import inventor_kit as ik
    data = path.read_bytes()
    def run():
        if operation == 'metadata':
            return ik.inspect(data)
        if operation == 'read':
            return ik.read(data)
        if operation == 'geometry':
            return ik.read(data).to_cadquery()
        return ik.read_assembly_file(path).to_cadquery(allow_unverified_state=True)
    start = time.perf_counter()
    result = run()
    cold = time.perf_counter() - start
    if operation == 'read' and result.model is None:
        raise ValueError('Benchmark requires a decoded part model')
    durations = []
    for _ in range(iterations):
        del result
        start = time.perf_counter()
        result = run()
        durations.append(time.perf_counter() - start)
    # ru_maxrss includes interpreter, imports and one result; it is not a delta
    # allocation count. macOS uses bytes; Linux uses KiB. Windows stays explicit.
    peak = None
    if sys.platform in ('linux', 'darwin'):
        import resource
        peak = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
        if sys.platform == 'linux':
            peak *= 1024
    return dict(operation=operation, file=path.name, sha256=hashlib.sha256(data).hexdigest(),
                input_bytes=len(data), iterations=iterations, cold_seconds=cold,
                median_seconds=statistics.median(durations), max_seconds=max(durations),
                process_peak_rss_bytes=peak, peak_rss_scope='process including interpreter/imports',
                native_module=ik._inventor.__file__,
                native_sha256=hashlib.sha256(Path(ik._inventor.__file__).read_bytes()).hexdigest())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixtures', type=Path, default=ROOT / 'fixtures/public')
    parser.add_argument('--output', type=Path, default=ROOT / 'internal/reports/latest/benchmark.json')
    parser.add_argument('--iterations', type=int, default=7)
    parser.add_argument('--goals', type=Path, help='Optional host-specific reference; no private baseline is required')
    parser.add_argument('--worker', choices=[c[0] for c in CASES])
    parser.add_argument('--file', type=Path)
    args = parser.parse_args()
    if not 1 <= args.iterations <= 100:
        parser.error('--iterations must be between 1 and 100')
    if args.worker:
        print(json.dumps(measure(args.worker, args.file, args.iterations), allow_nan=False))
        return
    from check_corpus import verify
    verify(args.fixtures)
    cases = []
    for row in CASE_ROWS:
        operation, name = row['operation'], row['file']
        command = [sys.executable, str(Path(__file__).resolve()), '--worker', operation,
                   '--file', str((args.fixtures / name).resolve()), '--iterations', str(args.iterations)]
        output = subprocess.check_output(command, text=True, timeout=120)
        case = json.loads(output)
        if case['sha256'] != row['sha256']:
            raise ValueError('Benchmark input differs from fixed public cases')
        cases.append(case)
    report = dict(schema_version=1, platform=platform.platform(), machine=platform.machine(),
                  processor=platform.processor(), cpu_count=os.cpu_count(), python=sys.version,
                  dependencies={n: importlib.metadata.version(n) for n in ('inventor-kit', 'cq-acis', 'cadquery')},
                  cases=cases, scope='local measurements, not universal latency/memory guarantees')
    if args.goals is not None:
        goals = json.loads(args.goals.read_text())
        comparable = all(report.get(k) == v for k, v in goals['reference_environment'].items())
        checks = []
        for goal in goals['cases']:
            found = [c for c in cases if all(c[k] == goal[k] for k in ('operation', 'file', 'sha256'))]
            if len(found) != 1:
                raise ValueError('Benchmark input differs from fixed reference goals')
            case = found[0]
            checks.append(dict(operation=case['operation'], file=case['file'],
                latency=case['median_seconds'] <= goal['median_seconds_max'],
                memory=None if case['process_peak_rss_bytes'] is None else case['process_peak_rss_bytes'] <= goal['process_peak_rss_bytes_max']))
        report['reference_goals'] = dict(environment_matches=comparable, checks=checks,
            status=('passed' if all(c['latency'] and c['memory'] for c in checks) else 'exceeded') if comparable else 'different_host',
            scope='observational; separate regression runs on the reference host establish goals')
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(dict(report=str(args.output), cases=len(cases))))


if __name__ == '__main__':
    main()
