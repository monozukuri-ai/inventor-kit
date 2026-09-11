# Supported scope

English | [日本語](support.ja.md)

`inventor_kit.capabilities()` returns the
[machine-readable capability table](../python/inventor_kit/capabilities.json).
It declares format support; it does not prove successful execution on a
particular host or successful CI on each operating system.

| Area | Validated scope | Unsupported or unverified |
| --- | --- | --- |
| IPT / IAM / IDW / IPN | CFB, OLE properties with resource limits, and selected thumbnails | Drawing, feature, and PMI semantics |
| Saved IPT geometry | Validated structures for RSeDb schema 31, Meta 8, and PmBRep major 19 / 25 / 26 / 28 / 31 | Extrapolation to adjacent versions or unknown layouts |
| Kernel | Observed SAB save versions 22000 / 22600 / 22700 / 22900 / 23200, zlib / zstd | All entities in each version; history reevaluation |
| Geometry output | Analytic surfaces and limited NURBS, trims, and tolerance data | Approximation of unsupported curves or surfaces |
| IAM | UFRx schema 15 / save major 31 with a fixed section-version sequence; saved references and placements | `.ipj` resolution, constraint solving, native colors |
| State | Saved candidates, provenance, diagnostics, and explicit unverified status | Determining the current Model State, visibility, or substitution state |
| STEP | XDE roundtrip checks for hierarchy, names, rigid placements, part RGB, and geometry metrics | Independent comparison against native Inventor state or colors |

The complete IAM section-version sequence is recorded in the capabilities JSON.
Acceptance requires header, index, and record structure validation in addition
to version checks. The library uses `MODEL_API_VERSION=2` and version 1 of the
document, candidate, and assembly APIs.

Geometry conversion handles planes, lines, circles and ellipses, cylinders and
cones, elliptical cylinders, selected sphere and torus forms, explicit NURBS,
limited subtype references, degenerate edges at cone apices, circular holes on
spheres, and coaxial sections of elliptical cones. ASM 22700 tolerance data is
read through partial views; only vertices that match stored endpoints within
the source tolerance are used. Unsupported subtypes, trims, and tolerant
edges/coedges retain their original data and diagnostics, and stop conversion.
Partial geometry is not returned as a complete solid.

Retaining history as original bytes does not establish semantic interpretation
or agreement with the current state. Returning multiple stored databases or
candidates is distinct from selecting the current state. Feature editing,
history reconstruction, constraint solving, PMI/GD&T, sheet metal unfolding,
and reading native display meshes are outside the supported scope.

The optional [local viewer](viewer.md) generates display meshes from successfully
converted IPT B-rep shapes and explicitly permitted IAM saved placements. IAM
retains the occurrence hierarchy and omission reasons; partial display requires
an additional opt-in. It also shows document information and saved previews.
It does not extend native geometry, assembly-state, or drawing support.
