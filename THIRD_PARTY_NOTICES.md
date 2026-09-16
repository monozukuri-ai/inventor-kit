# Licensing boundaries and third-party notices

## Earlier inventor-kit material

The original MIT notice, including its `cq-acis contributors` copyright line,
is preserved verbatim in `licenses/inventor-kit-legacy-MIT.txt`. It applies to
material previously offered under MIT, including releases 0.1.0 and 0.2.0 and
public source at `440f149fe96a4c8e7f207ce621919284f6a1f621`. The origin of that
collective attribution has not been established; it is retained rather than
reassigned. The new license notice identifies UnRobotics Inc. as licensor,
not as the asserted owner of all existing contributions.

The separate cq-acis crates are dependencies, not part of this relicensing.
No additional rights in third-party code, data, trademarks or patent claims
are created by an inventor-kit commercial agreement.

## Format and implementation references

- cadmpeg project, commit `faa73bfdaa29a7b4fb5c6998c1bdedfec8fac9d7`:
  https://github.com/cadmpeg/cadmpeg/tree/faa73bfdaa29a7b4fb5c6998c1bdedfec8fac9d7
  RSe registry/meta tables/record trailers and typed carrier selection were
  implemented using its format notes and Apache-2.0 reference source.
  The UFRx/AmDc/AmGraphics reader also uses its external_reference.rs and
  assembly.rs as layout references. The independently written reader admits only
  the observed major-31 profile, whose additional fields differ from those
  reference files' synthetic fixtures. Filesystem resolution and graph expansion
  are implemented here. No GPL InventorLoader code is incorporated.
  The ASM format notes are CC-BY-4.0 (https://creativecommons.org/licenses/by/4.0/).
  The license text is preserved in `licenses/CC-BY-4.0.txt`.
  Attribution: the cadmpeg project. These are adapted notes and a smaller independently written reader, not a complete port.
  The Apache license copy is `licenses/cadmpeg-Apache-2.0.txt`. This project implements a smaller reader
  and adds zstd envelopes checked by adversarial tests and public files and bounded profile gates.
- cq-acis (monozukuri-ai), MIT: the shared Rust model and SAB reader use
  crates.io `acis-core`; the Python conversion uses `acis-py-bridge`.
  https://github.com/monozukuri-ai/cq-acis
  License: `licenses/cq-acis-MIT.txt`.
- ezdxf, Manfred Moitzi, MIT: SAB tags and independent fixture/token oracle.
  https://github.com/mozman/ezdxf
  License: `licenses/ezdxf-MIT.txt`.
- encoding_rs 0.8.35, Henri Sivonen and contributors: explicit code-page decoding.
  Used under MIT plus the BSD-3-Clause WHATWG data license. Notices:
  `licenses/encoding_rs-MIT.txt`, `licenses/encoding_rs-WHATWG.txt`,
  `licenses/encoding_rs-COPYRIGHT.txt`.
- crc32fast, MIT: PNG chunk CRC checking. License: `licenses/crc32fast-MIT.txt`.
- sha2 0.10.9 and its supporting Rust dependencies: content-bound candidate IDs
  and source fingerprints. Used under MIT; notices are preserved in
  `licenses/sha2-dependencies-MIT.txt`.
- MS-OLEPS and the W3C PNG specification informed the independent document reader.
  Exact sections and limitations are linked from `docs/format-reference.md`.
  olefile and Pillow are validation dependencies only; their source is not copied.

- The optional viewer uses ocp-tessellate 3.5.1 (Apache-2.0), distributed as a
  separate Python dependency: https://github.com/bernhard-42/ocp-tessellate.
  Its tessellator and B-rep edge discretization are called through an adapter.
- The bundled viewer includes three-cad-viewer 5.0.6, Three.js, n8ao and
  postprocessing (MIT). Exact bundled versions and complete license texts are
  in `python/inventor_kit/viewer/static/THIRD_PARTY_LICENSES.txt`, also installed
  under `inventor_kit/viewer/static/`. Build inputs and asset hashes are in the
  adjacent `manifest.json`. https://github.com/bernhard-42/three-cad-viewer

Source provenance and sample hashes are recorded in `fixtures/manifest.json` and
`fixtures/assembly-manifest.json`. GPL InventorLoader code is not incorporated.

## Native dependency notices

`licenses/rust-dependencies.txt` preserves the license and copyright materials
from the locked Cargo dependencies, including target-specific and build tools.
`licenses/rust-dependencies.json` records their versions, selected license paths
and source checksums. The catalog includes build tools for attribution; it does
not claim that every listed tool is linked into every wheel. MIT is selected
where the upstream offers it as an alternative; mandatory data licenses and
the BSD-3-Clause zstd C library license are retained as well.

The distribution SPDX expression accounts for new PolyForm material, earlier
MIT material and bundled MIT JavaScript, Apache-2.0 and CC-BY-4.0 reference
material, BSD-3-Clause data/zstd, and the Zlib implementation. These conditions
apply to the corresponding portions, not as a choice of license for the whole
project. The expression does not automatically incorporate separately installed
Python dependencies.

CadQuery, cadquery-ocp/OCCT and other Python dependencies retain their licenses.
An OEM redistribution that bundles them must also supply the notices and any
source or relinking materials required by those components; a package's top-level
metadata alone does not describe all native libraries inside its wheel.
