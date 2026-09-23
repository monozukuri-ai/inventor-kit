# inventor-kit v0.5.0

English | [日本語](release-notes-v0.5.0.ja.md)

v0.5.0 adds saved IDW drawing display, reports and offline SVG export to the
existing IPT/IAM workflows. Drawing support is experimental and partial.

## Drawing workflows

- Read saved sheets, views, supported curves, text, styles and embedded images
  with `read_drawing_file`. The observed profiles are schema31 / Meta8 with
  major23 (zlib) or major31 (zstd).
- Open supported IDW files in the Viewer by default, with sheet selection,
  pan/zoom, text search and source inspection. IDW display does not require the
  Python `viewer` extra. The old `--experimental-drawing` flag remains accepted.
- List sheets, export JSON reports and process multiple reports as JSONL from
  the CLI. Per-input timeouts and classified exit codes remain available.
- Export a selected sheet through the CLI, Python API or Viewer using the same
  SVG renderer. Images are embedded for offline viewing; the JSON sidecar records
  input/output hashes, original text, provenance and omissions. Existing outputs
  are never overwritten. SVG export requires explicit partial-output consent.

## Rendering and platform fixes

- Render circles and elliptical arcs with SVG arc commands. Nearly degenerate
  ellipses retain a recorded bounded line approximation.
- Decode nominal major31 patterns for 15 built-in line styles, including layer
  inheritance and observed line-width scaling, from 38 saved Inventor controls.
  Layer/binding scales are limited to the observed value of 1. Explicit saved
  pattern arrays already include their scale.
- Improve Japanese and diameter-symbol font fallbacks, including macOS fonts.
- Retry transient Windows sharing/access errors while reading replaced Viewer
  snapshots; persistent failures remain errors.

## Installation and release checks

```sh
python -m pip install --upgrade inventor-kit==0.5.0
```

Python 3.11 or later is required. The dependency contract remains
`cq-acis>=0.3.8,<0.4`; existing IPT/IAM body selection and STEP workflows remain
available. Licensing remains as described in the [license guide](license.md).

The release workflow requires four wheels (Linux x86_64, Windows x86_64,
macOS Intel and Apple Silicon) and an sdist. It checks nine isolated installations
across Python 3.11/3.12, including an independent sdist rebuild. Chromium checks
on each platform render five fixture sheets and one font/dash control from an
installed wheel. These checks cover SVG display, not physical GPUs or Safari.

## Known limits

- Drawing results remain `experimental_partial`, `qualified=False` and
  `complete=False`; current state and physical units remain unverified. SVG
  coordinates do not certify millimeters, measurements or physical printing.
  The millimeter-based `render_sheet` entrypoint still rejects these profiles.
- Saved coordinates match API values for four unit-control inputs only. Their
  native PDFs do not meet the 0.001 mm comparison threshold; this evidence does
  not qualify arbitrary IDW files.
- Native PDF dash fitting and curve phase are not reproduced. SVG output retains
  nominal patterns and reports `dash_phase_and_fit_unverified`. General major23
  layer styles, custom `.lin` files and unobserved scales remain unqualified.
- Unsupported drawing elements retain omissions. Native font fidelity, complete
  annotation semantics and model reprojection are not provided. Fonts are not
  embedded in SVG; major23 spline display remains an approximation.
- Successful partial IDW reports and SVG exports return CLI exit code **2**.

See the [drawing guide](drawing.md) and [CLI examples](cli.md) for API contracts,
resource limits and output details.
