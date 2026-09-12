# Validation scope and reproducibility

English | [日本語](validation.ja.md)

Container parsing, candidate selection, typed parsing, geometry construction,
and comparison against the current state are validated separately. Retaining
values or original bytes alone does not establish correct interpretation of the
entire model.

The [part manifest](../fixtures/manifest.json) and
[assembly manifest](../fixtures/assembly-manifest.json) record public sample
sources, license conditions, sizes, and SHA-256 hashes. Together they contain
45 entries, including four holdouts. They include related models and duplicate
content, so they do not represent 45 independent model families. Download scripts
retrieve the CAD files; the files are excluded from Git and distributions.

## Published results

The [validation summary](../reports/validation-summary.json) contains only the
execution environment category, test counts, corpus counts by stage, and fuzz
results. It excludes local paths, detailed diagnostics, raw property or model
data, and execution logs. The fixed part corpus has 33 inputs, including 28 IPT
files and five other document types. Saved tables were parsed in 28 files and
10 files were converted to valid solids (regression: 9/30;
holdout: 1/3). Diagnostics for unsupported cases are also regression checks.
The holdout IAM remains an unsupported profile and is not used to tune acceptance
criteria.

These results cover a limited set of public samples. They do not establish
agreement with the current state or vendor implementation, or successful
distribution CI on Windows and macOS.

For the dependency update on 2026-09-10, published `acis-core` / `acis-py-bridge` /
`cq-acis` 0.3.2 passed 41 Rust tests and 68 Python tests with zero skips on Linux.
Fresh Python 3.11 / 3.12 environments passed wheel installation, saved part
geometry conversion, assembly STEP roundtrips, and normal subprocess shutdown.
The sdist was also rebuilt in locked/offline mode without local path patches,
then installed and validated with public PyPI dependencies. Release verification
on other operating systems is performed separately through distribution CI.

## Comparison methods

| Comparison | What it checks | Limits |
| --- | --- | --- |
| Fixed inputs and frozen geometry metrics | Regressions in volume, area, solid count, and bounding boxes | Reference values originate from the same converter |
| Analytic cylinder and box formulas | Dimensions and volume/area in millimetre-based units | Not an oracle for complex models or the current state |
| olefile / Pillow | Supported property values and independent PNG decoding | Unsupported comparator cases are not counted as matches |
| ezdxf | SAB record boundaries and tags | Does not establish geometry or saved-state agreement |
| STEP / XDE roundtrip | Preservation of hierarchy, names, placements, part RGB, and geometry metrics | Does not verify native Inventor state or colors |
| Apprentice / Inventor captures | Independent comparison of fields captured from the same input and state | Requires an actual provider capture and provenance; uncaptured fields remain unverified |

The frozen geometry baseline contains regression inputs only; holdouts are not
added to the fitted baseline. Manifests maintain the policy of reserving new
sources or model families as holdouts. Missing inputs, hash mismatches, and
skipped tests are not treated as success.

## Reproduction and optional oracles

See the [development guide](development.md) for standard reproduction steps and
the [benchmark guide](../benchmarks/README.md) for performance measurement.

```sh
python scripts/summarize_validation.py --input internal/reports/latest --output reports/validation-summary.json
```

Summary generation fails if required detailed results are missing. CI publishes
only a summary selected in the same way; it does not upload raw execution results.

On Windows, `capture_vendor_oracle.py` reads through Apprentice, while
`capture_model_states.py` uses Inventor itself to switch states on a copy of the
input. The latter saves only the copy and records whether state switching and
updating succeeded. `compare_state_candidates.py` compares only candidates whose
input hashes and capture evidence match. Synthetic oracles test the comparison
pipeline and do not count as validation against an actual Autodesk installation.
Explicitly choose an output location under `internal/` for captures.
