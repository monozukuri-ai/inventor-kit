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
12 files were converted to valid solids (regression: 11/30;
holdout: 1/3). Diagnostics for unsupported cases are also regression checks.
The holdout IAM remains an unsupported profile and is not used to tune acceptance
criteria.

These results cover a limited set of public samples. They do not establish
agreement with the current state or vendor implementation, or successful
distribution CI on Windows and macOS.

On 2026-09-14, public `acis-core` / `acis-py-bridge` / `cq-acis` 0.3.7
passed 42 Rust tests, 87 Python tests and 29 viewer integration tests with zero
skips on Linux. FTC07 (2021) retains its valid 258-face solid. The focused closure
gate produced these results with the published dependencies:

| Input | Valid individual faces | Primary solid | Complete saved-part conversion |
| --- | --- | --- | --- |
| FTC06 (2021) | 148 / 148 | closed, valid, 146 faces | rejected: two auxiliary open planar bodies |
| FTC06 (2024) | 148 / 148 | closed, valid, 146 faces | rejected: two auxiliary open planar bodies |
| CTC04 (2021) | 368 / 368 | closed, valid, 368 faces | converted; viewer includes every face |

The primary solids passed STEP roundtrips with matching face counts, volume,
area and bounding boxes. FTC06 is excluded from the complete-conversion count.
Distribution installation and browser checks run separately as described in the
[release guide](releasing.md).

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

The focused analytic-closure gate checks FTC06 (2021/2024) and CTC04 (2021):

```sh
python scripts/validate_analytic_closure.py
```

It requires the qualified cylinder-seam and shell-closure implementation in
`cq-acis>=0.3.7,<0.4`, plus the viewer dependencies. It fails explicitly if those
converter diagnostics are absent. The gate verifies source face provenance, all individual
faces, closed primary bodies, STEP roundtrips, mesh coverage and viewer behavior.
FTC06 includes two open auxiliary planar bodies: their individual previews and
the primary solid are reported separately, and complete-solid conversion must
still reject the file. The CTC04 check requires complete saved-solid conversion
and viewing. Cone apex normals use the analytic limit at the tessellation's UV
angle; positions and triangles remain unchanged. These checks do not establish
the current Model State or vendor equivalence, and use regression inputs only.

The optional FTC10/CTC02 trim gate requires the **unreleased** cq-acis
`cylinder_slit_faces` and elliptic-section diagnostics:

```sh
python scripts/validate_revolution_trims.py
```

It checks four repaired FTC10 (2021) faces, including face 1387's three paired
source edges, and six oblique CTC02 (2021) sections. The gate retains all source
vertices through STEP, checks edge counts and mesh coverage, and requires
whole-part conversion and automatic 3D preview to reject the still unsupported
curves/surfaces. The local implementation has 198/223 valid FTC10 faces and
411/443 valid CTC02 faces; neither file is counted as a complete solid.
Public cq-acis 0.3.7 does not pass this optional gate. It is not enabled in CI
until the shared dependency is released and adopted.

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
