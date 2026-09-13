# Tolerant boundaries and finite UV

English | [日本語](tolerant-trims.ja.md)

The normal and fuzz Rust graphs now use the public crates.io releases
`acis-core=0.3.4` and `acis-py-bridge=0.3.4`; Python requires
`cq-acis>=0.3.4,<0.4`. Shared model API 2 is unchanged. Public 0.3.4 provides
finite spline domains, saved degree-1/3 UV views, independent curve/support
senses and bounded TEDGE deviations. Publication and the development additions
below are separate: the latter require matching cq-acis sources.

## Reproducing the checks

```sh
python scripts/validate_components.py
python scripts/validate_topology.py
python scripts/validate_tolerant_trims.py
python scripts/validate_geometry.py
```

Scripts verify fixture hashes and write reports under `internal/reports/latest/`.
Topology and trim checks use the FTC07 2021 regression fixture; the whole
geometry check keeps the independent holdout split and frozen solid metrics.
The trim report records package versions, loaded modules, raw hashes, source
spans, subtype ownership, applied bounds and measured deviations. Its assertions
select the public 0.3.4 or development profile by additive native capability.

## Unreleased inline and associated curves

The new core/bridge view decodes the observed full `par_int_cur` in all ten
inline coedges, retaining the cubic 3D fit, support, saved degree-3 or rational
quadratic UV and source extent. Plane UV scale and sphere/torus angle order are
explicit. The converter independently checks the inline fit against its own
support and the unchanged shared edge against its TEDGE bound. For two rational
planar arcs, a whole-curve conic identity and monotonic minor-arc certificate
allow the analytic edge parameterization; saved poles remain in the raw model.

Qualified inline uses also bound source tolerant endpoints and exact-ID UV
joins. Ordinary lines may narrow their interval to source points on the same
line inside the original bounds. Planar hole orientation retains every edge.
No global resolution, finite UV domain, shared 3D curve or unknown TVERTEX
scalar is changed to make a failing comparison pass.

`NativeModel.supported_curve()` decodes the full `int_int_cur` at curve 4022,
including its file-local spline support, secondary plane, saved UV and trailer.
The unchanged 3D fit is checked over its full domain against both supports;
the saved UV's original same-parameter discrepancy is reported separately.
This establishes per-support bounds, not an exact intersection. Its endpoint
still fails the original source-vertex precision on two incident faces.

## Local results (2026-09-13)

| Check | Public 0.3.4 | Development sources |
| --- | ---: | ---: |
| Decoded inline coedges | 0 / 10 | 10 / 10 |
| Valid tolerant coedge boundaries | 195 / 206 | 206 / 206 |
| Valid individual FTC07 faces | 245 / 258 | 254 / 258 |
| Valid finite-UV faces | 5 / 8 | 5 / 8 |
| Readable finite spline supports | 8 | 8 |
| Saved spline pcurve views | 224 | 224 |

All previously valid 245 face areas are unchanged. The nine newly valid face
IDs are 1524, 1535, 1994, 1997, 2782, 2822, 2977, 2998 and 4162. The 33-file
corpus retains ten valid solid conversions (nine regression and one holdout),
with all frozen regression solid metrics preserved.

Four faces remain rejected:

| Faces | Source failure | Measured / allowed (mm) |
| --- | --- | --- |
| 1164 | saved pcurve / TEDGE mismatch | 0.006255113958 / 0.006130569677 |
| 2336 | saved pcurve / TEDGE mismatch | 0.006246879672 / 0.006109190844 |
| 332, 4351 | curve 4022 / vertex 3618 endpoint mismatch | 0.006526083452 / 0.000010 |

Reparameterization trials do not qualify the two saved pcurve mismatches under
the original edge bounds. Projected alternatives leave the finite UV domain,
so they are not adopted. The spline endpoint discrepancy exposes an additional
failure previously hidden by unsupported inline/curve handling. Unknown vertex
fields do not supply an allowance. Complete FTC07 conversion still rejects with
`geometry.pcurve_mismatch`; successful components are not a valid complete part.
These are local source-consistency checks, without Inventor/current Model State,
Windows/macOS or remote CI qualification.
