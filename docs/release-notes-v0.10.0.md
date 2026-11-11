# inventor-kit v0.10.0

English | [日本語](release-notes-v0.10.0.ja.md)

## Shape conversion is an optional extra

inventor-kit now depends on `cq-acis>=0.4.2,<0.5`, which no longer installs
CadQuery by default. Choose one shape backend:

```sh
python -m pip install 'inventor-kit[cadquery]'  # CadQuery shapes, STEP export, IAM, writers
python -m pip install 'inventor-kit[occt]'      # OCCT shapes only, without CadQuery
```

**Breaking:** a plain `pip install inventor-kit` installs the parser and the
shared model only. Existing uses of `to_cadquery()`, STEP export, assemblies and
the writers need `inventor-kit[cadquery]`. `inventor-kit[viewer]` includes it.
The two extras ship different builds of the same `OCP` module; install one.

## OCCT backend

- `doc.convert_bodies(backend="auto" | "cadquery" | "occt")`. With `"occt"`,
  `body.shape` is an OCCT `TopoDS_Shape` and CadQuery is not imported. Statuses,
  diagnostics and metrics are identical on both backends; `result.backend`
  names the one used.
- `result.to_occt(body_ids=..., allow_partial=...)` and `doc.to_occt()` return
  `TopoDS_Shape` objects. `to_cadquery()` and `export_step()` on an OCCT result
  raise `ValueError`.

## Stored B-spline fits

Procedural intersection curves and blend, loft, sweep and revolved spline
surfaces are converted from the B-spline fit that Inventor stores beside them
(cq-acis 0.4.1 and 0.4.2). Bodies that failed with
`geometry.curve_unsupported` or `geometry.surface_unsupported` can now convert.

Such a body is **derived geometry** within the stored fit tolerances. It carries
`body.warnings` with the code `geometry.approximated_from_stored_spline`, and the
conversion report, the CLI and the viewer show the warning. `body.approximations`
lists the fits. The library still computes no approximation of its own, and
procedural geometry without a stored fit, such as helical thread surfaces,
remains unsupported.

The conversion report schema v1 gains an optional `warnings` array per body.

## Candidate segment major

PmBRep segment major 22 with SAB save version 22300 is read as a **candidate**
profile: it was observed and is framed like the validated majors, but has no
pinned regression fixture. Its table reports
`profile_status == "candidate_stored_table"` and the warning
`candidate.profile_candidate`. Validated majors (19 / 25 / 26 / 28 / 30 / 31)
still report `supported_stored_table`; other majors remain unsupported.
`capabilities()` lists `candidate_segment_majors`.

## Requirements

Python 3.11 or later. Rust sources use `acis-core=0.4.2` and
`acis-py-bridge=0.4.2`.
