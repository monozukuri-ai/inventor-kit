# Benchmarks

English | [日本語](README.ja.md)

[cases.json](cases.json) defines operations, paths, and SHA-256 hashes for five
fixed public regression samples. Retrieve the CAD files using the
[development guide](../docs/development.md); they are not bundled here.

```sh
python scripts/benchmark_parser.py --output internal/reports/latest/benchmark.json
```

Each case runs in a separate process for one initial call and seven subsequent
calls. Measurements record cold time, warm median and maximum time, and process
peak RSS. Part API timings exclude reading the input file and importing
inventor-kit; file resolution within the assembly API is timed. The first import
of geometry modules is included in cold time. RSS is the process peak, including
Python and dependencies. Measurements normalize Linux KiB and macOS bytes to a
common unit; Windows RSS measurement is unsupported and returns null.

Results are observations on a particular host, not general guarantees of speed
or memory use. Optional host-specific targets can be supplied with `--goals <JSON>`.
Public cases do not depend on local target files or previous measurements. Store
detailed results, host information, and targets in `internal/benchmarks/` or
`internal/reports/`.
