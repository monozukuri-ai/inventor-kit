# Validation scope and reproducibility

English | [日本語](validation.ja.md)

Container parsing, candidate selection, typed parsing, geometry construction,
and comparison against the current state are validated separately. Retaining
values or original bytes alone does not establish correct interpretation of the
entire model.

The [part manifest](../fixtures/manifest.json) and
[assembly manifest](../fixtures/assembly-manifest.json) record public sample
sources, license conditions, sizes, SHA-256 hashes and holdout assignments.
Related models and duplicate content do not count as independent model families. Download scripts
retrieve the CAD files; the files are excluded from Git and distributions.

## Validation results

Validation scripts write detailed results under `internal/reports/latest/`.
The summary command below selects counts and statuses and writes
`internal/reports/validation-summary.json`. Keep comparison records and logs
under the same `internal/reports/` directory; they are not bundled with the library.
Results from one environment or dependency version do not qualify other platforms
or the current Inventor state.

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

The FTC10/CTC02 trim gate runs in CI with published cq-acis 0.3.8
`cylinder_slit_faces` and elliptic-section diagnostics:

```sh
python scripts/validate_revolution_trims.py
```

It checks four repaired FTC10 (2021) faces, including face 1387's three paired
source edges, and six oblique CTC02 (2021) sections. The gate retains all source
vertices through STEP, checks edge counts and mesh coverage, and requires
whole-part conversion and automatic 3D preview to reject the still unsupported
curves/surfaces. This gate requires cq-acis>=0.3.8,<0.4. Per-face results do not
establish complete-part support. The generated report records measured counts.

See the [development guide](development.md) for standard reproduction steps and
the [benchmark guide](../benchmarks/README.md) for performance measurement.

```sh
python scripts/summarize_validation.py --input internal/reports/latest --output internal/reports/validation-summary.json
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

See the [IDW guide](drawing.md) for drawing behavior and support limitations.
