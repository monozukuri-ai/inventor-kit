# Drawing corpus and comparison captures

[日本語](drawing-validation.ja.md)

This is validation infrastructure for future IDW reading/display. It does not
add a drawing decoder or viewer. Native Autodesk captures and controlled
one-change IDWs have **not been acquired**. The Windows collector has been
checked with synthetic COM-shaped objects only.

## Corpus

`fixtures/drawing-manifest.json` selects five drawings. The two existing
MetaReader inputs reference canonical rows in `fixtures/manifest.json`; their
identity, family and split are not duplicated. New assets have pinned repository
commits, paths, byte sizes, SHA-256 and upstream license notices.

| Selection | Split / family | Available material |
| --- | --- | --- |
| `_Fishing Rod Assembly.idw`, `SampleBg.idw` | regression / `metareader-samplebg` | Existing IDWs |
| `Template_IACS.idw` | regression / `iacs-template` | Template and MIT license |
| `RIM.idw` | holdout / `forge-rim` | Drawing, part and MIT license |
| `VISE ASSEMBLY.idw` | holdout / `lfrum-vise` | Drawing, assembly/parts, PDF and GPL-3.0 license |

Part/assembly roles follow upstream project descriptions and file associations;
API classification, simplicity and reference completeness remain unverified.
The vise PDF has the same stem at the same commit, but its binding to the saved
IDW state is **unverified**. It is not an independent ground-truth capture.
New files are downloaded into ignored `fixtures/public/drawings/`; CAD/PDF/license
payloads are excluded from distributions. No upstream source code is incorporated.
The pinned upstream license documents travel with each downloaded project.

The holdouts reserve entire project families, including references and visuals.
Do not tune the decoder or tolerances against them. If used for development,
move the whole family to regression and acquire another independent holdout.
Shared validation rejects family/hash leakage and duplicate paths across all
three manifests, including case aliases and unsafe relative paths.

```bash
python scripts/fetch_public_samples.py
python scripts/fetch_assembly_samples.py
python scripts/fetch_drawing_samples.py
python scripts/check_corpus.py
python scripts/validate_drawing_oracle.py --inventory
```

`check_corpus.py` checks all 66 declared assets, including licenses/references,
not 66 IDWs. `--inventory` reports source integrity and acquisition separately:
five drawings, zero native captures, acquisition `incomplete` initially.
Its exit code checks source integrity only. Without `--inventory`, missing
captures return exit code 1. CI runs offline tests after downloading fixtures;
it neither starts Autodesk nor claims native acquisition passed.

## Windows acquisition

From a repository checkout with the `oracle` extra installed, and a compatible
Autodesk installation, use a **new** output filename:

```powershell
python scripts/capture_drawing_oracle.py --provider apprentice --input "fixtures/public/drawings/rim/RIM.idw" --project-root "fixtures/public/drawings/rim" --output "internal/oracles/rim.json"
```

`--provider inventor` uses a separate Inventor COM application and
`OpenWithOptions(DeferUpdates=True, SkipAllUnresolvedFiles=True, OpenVisible=False)`.
Apprentice uses its drawing document API. Both work on a temporary copy and make
no Save/Update call. `--project-root` copies the selected tree with relative paths
preserved; without it only the IDW is copied. The copy is bounded to 10,000 files /
512 MiB; symbolic links are refused. Keep project roots small and self-contained.
Original and copied file hashes are checked after closing. External resolved
references are reported with unavailable identity instead of being read by the
collector. Autodesk itself controls reference resolution; completeness and
opening-time updates are not assumed.

Captures record `scope=opened_document`, observed Dirty/RequiresUpdate/DeferUpdates
before and after queries, `automatic_update=unknown`, provider build, collector
hash, input identities and query errors. No-save does not prove no migration or
in-memory update. Preserve independent session evidence before certifying
`automatic_update=not_occurred`; an updated open state cannot qualify as the
original saved-state baseline. No output is overwritten.

## Contract and offline validation

The separate `schemas/drawing-oracle-v1.schema.json` leaves the existing 3D
oracle contract unchanged. Collections record observed counts, ordered 1-based
indices and per-item results. Failed queries have reasons and no `value`;
`Nothing` is unavailable, while Count=0 is a captured empty collection.
Partial enumeration is preserved. Duplicate JSON keys, non-finite numbers,
invalid counts/order, source mismatches and artifact identity mismatches fail.
When a source is supplied, its recorded relative path identifies the project root;
all staged input files, including copied dependencies, must still match there.

The bounded collector queries:

- Sheet order, names, dimensions, size/orientation/status; view positions, scale
  and rotation.
- Flattened drawing-curve segments: line, circle and circular arc geometry,
  visibility, hidden-line flag and layer. Unsupported types remain explicit.
- Sheet/view sketches in local coordinates, and images of local basis points
  `(0,0)`, `(1,0)`, `(0,1)` in sheet space; basis interpretation still needs native
  qualification. Sketch points and other unsupported entities remain explicit.
- General notes and sketch text, including original/formatted strings, placement,
  rotation, sizes and style font; dimension display strings and origins;
  evaluated title-block/border strings and parts-list cells.

Lengths remain database centimetres and angles radians. The collector does not
export images or collect leader notes, symbols, hatches, images, border geometry,
or table layout. Unsupported API surfaces in Apprentice remain unavailable/failed.
The JSON size is bounded to 32 MiB and collection queries to 100,000 total items.

```bash
python scripts/validate_drawing_oracle.py --capture internal/oracles/rim.json --source fixtures/public/drawings/rim/RIM.idw
# Acquisition gate: fails for synthetic, unknown saved state, missing fields/visuals.
python scripts/validate_drawing_oracle.py --capture internal/oracles/rim.json --source fixtures/public/drawings/rim/RIM.idw --require-ready
# Corpus gate: each capture is named <IDW-sha256>.json; absent captures fail.
python scripts/validate_drawing_oracle.py --captures internal/oracles
```

Ordinary single-capture validation certifies the contract and source identity,
not provider authenticity or drawing correctness. `--require-ready` additionally
requires native observations, known unchanged saved state, complete captured
queries/references and a same-session visual for every sheet. This conservative
acquisition gate is not a decoder or visual-difference test.

`visual_artifacts` accepts relative PDF/PNG paths, bytes/hash, source hash, sheet
index, provenance, binding, DPI and font conditions. Files are checked against the
capture directory (`--artifact-root` overrides it), including media signatures.
Independent Windows export, session binding, rasterization DPI and font evidence
must be supplied before visual comparison; the collector emits no artifacts.
Do not relabel the public vise PDF or a synthetic image as `same_session`.

`tests/data/drawing-oracle-synthetic.json` and fake COM objects exercise the
contract and failure handling only. They cannot satisfy native acquisition.
See the [controlled specimen procedure](drawing-specimens.md) for the remaining
Windows work.

## API sources

The queried surfaces follow Autodesk's documentation:
[Apprentice drawing document](https://help.autodesk.com/cloudhelp/2024/ENU/Inventor-API/files/ApprenticeServerDrawingDocument.htm),
[open options](https://help.autodesk.com/cloudhelp/2024/ENU/Inventor-API/files/Documents_OpenWithOptions.htm),
[sheet-space curve geometry](https://help.autodesk.com/cloudhelp/2024/ENU/Inventor-API/files/DrawingCurveSegment_Geometry.htm),
[sketch-to-sheet mapping](https://help.autodesk.com/cloudhelp/2024/ENU/Inventor-API/files/DrawingSketch_SketchToSheetSpace.htm),
[database units](https://help.autodesk.com/cloudhelp/2021/ENU/Inventor-API/files/UOM_Overview.htm),
[evaluated title-block text](https://help.autodesk.com/cloudhelp/2024/ENU/Inventor-API/files/TitleBlock_GetResultText.htm).
These API descriptions do not establish collector behavior on an untested install.

[Structural inventory](drawing-inventory.md) provides a bounded Rust entry point without claiming drawing semantics.
