# Format and implementation references

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

Source provenance and sample hashes are recorded in `fixtures/manifest.json` and
`fixtures/assembly-manifest.json`. GPL InventorLoader code is not incorporated.
