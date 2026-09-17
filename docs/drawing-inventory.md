# IDW structural inventory

[日本語](drawing-inventory.ja.md) · [Drawing corpus](drawing-validation.md)

`inventor_core::drawing::inspect(bytes, source_id, limits)` returns document
metadata, bounded RSe framing, and **provisional typed payload fields**. Field
roles are unqualified. An opt-in experimental scene now applies stored
placements and selected display primitives; canonical sheet selection, qualified
rendering, and native Autodesk comparison remain unavailable. An [experimental Python API and Viewer](drawing.md) now expose the stored sheet list and linked display spaces; physical units and native accuracy remain unverified.

From a repository checkout:

```bash
cargo run --locked -p inventor-core --example inspect_drawing -- fixtures/public/SampleBg.idw
python scripts/validate_drawing_inventory.py
```

The example emits one JSON object per input. The validation script builds that
example, checks the three regression IDWs against their source hashes and a
structural snapshot, and independently checks reported ranges using olefile and
zlib/zstandard. Reports go to `internal/reports/latest/drawing_inventory.json` by
default. Holdout drawing contents are excluded from this validation and fuzz seeds.
No drawing files are required by the Rust unit tests; those use synthetic CFBs.

## Admitted framing profile

| Boundary | Requirement |
| --- | --- |
| Document | Identified drawing root CLSID, no conflicting document kind |
| Database | Exactly one completely decoded schema-31 RSeDb |
| Ownership | Unique document-namespace registry ID/Meta ID and matching names |
| Segment major | 31 only; major23/24 retain identity and unsupported diagnostics |
| Segment kinds | `DlDocDcSegmentType`, `DlBRxSegmentType`, `DlDirectorySegmentType`, `AppSegmentType`, `FBAttributeSegment`, `DlSheetDcSegmentType`, `DlSheetSmSegmentType`, `DlSheetDlSegmentType` |
| Meta | Version 8; DocDC/SheetDC section 9 uses 15-byte entries, other admitted kinds use 19; DocDC allows up to 4096 descriptors, others 256 |
| B stream | Observed 18-byte envelope `9ec22ba4d411e80160002db3ee29fbb00402`, followed by exactly one zstd frame |
| Records | Active Meta block slots, low-byte type selector, declared payload lengths, supported extended trailers, terminal marker |

The profile name is `idw-rse31-meta8-major31-zstd-framing-v2`. Matching a segment
kind/major does not waive Meta/B grammar checks. A changed header, codec, table
variant or trailer fails that segment. Meta uses the shared bounded zlib/zstd
reader. Opaque suffix bytes after the bulk terminal marker are retained as ranges;
their internal structure and completeness are not certified.

Only `/RSeStorage/M*` streams can own document segments. Templates and other
nested namespaces are opaque, even if their names/IDs resemble document objects;
their different Meta headers are not parsed with the document grammar. All such
streams are still counted against budgets and reported. Orphan B streams never
supply guessed records. Duplicate document IDs, unknown document Meta identities,
name conflicts and multiple databases prevent the affected ownership claim.

## Output and evidence

The report contains the original metadata/properties/thumbnails, source SHA-256,
registry entries, identity joins, admitted profile, Meta block words and full type
GUIDs, compression information, record ordinals, diagnostic codes, unclaimed
streams, opaque ranges and `observations`. `sheet_count` is always null and `drawing_semantics`
is `not_decoded` for qualified document semantics; provisional fields do not change
this status. Names such as `DLSheet128...` are not proof of sheet count/order.

Offsets are half-open `[start_offset,end_offset)` ranges in the named CFB stream
or its inflated byte domain, **not physical file offsets**. A compressed source
range links each inflated Meta/B stream to its original bytes. Record ordinals
and type indices are zero-based; inactive Meta block slots keep their ordinal
positions. Selector high bits are retained without interpretation. A full type
GUID is preserved even when two GUIDs share the same leading bytes.

Trailer reference candidates contain only framed `raw_name` / `raw_value` pairs,
their source ranges and `unresolved_not_followed`. Payloads are never scanned for
numbers, text or signatures to infer references. Candidates are not resolved or
traversed, so repeated/cyclic values cannot trigger recursive loading. The rest of
the payload/trailer semantics remain explicitly opaque.

`framed_subset` means all registry entries passed the admitted framing grammar;
it still excludes templates and payload semantics. `partial` means some entries
framed and others failed. `identity_only` means no records were admitted. Invalid
CFB/file limits return an error; an unsupported drawing profile returns metadata
and diagnostics. A failed record table never leaks a partially accepted record
list from that segment.

## Bounds and local evidence

Existing `Limits` bound file/stream sizes and metadata. The drawing inventory adds
aggregate accounting within that contract: all declared RSe bytes, including
ignored Templates; emitted expansion allowance across Meta and B streams; and
work items across registry objects, identity queries, Meta tables, record slots,
trailer properties, reference entries, typed UTF-16 code units and numeric fields. Each document Meta is cached once and
a uniquely owned pair is inflated at most once. Failed parsing keeps its spent
budget. A decoder error with unmeasurable output conservatively exhausts the
remaining expansion allowance. These counters concern RSe inventory work; the
existing metadata/property inspection retains its own limits.

Current regression observation: `SampleBg.idw` frames **5733 records in all 8
segments**, including 4749 DocDC and 153 SheetDC records. DocDC contains 303 type
descriptors but observed selectors address only indices 0–197; descriptors above
255 are preserved, not made addressable by widening the selector. Both DC reverse
chains join their forward type-table footer with exactly 15 bytes per section-9
entry. Other segment kinds and the existing IPT/IAM Meta grammar are unchanged. `_Fishing Rod Assembly.idw` (major23) and `Template_IACS.idw`
(major24) remain identity-only. These are parser regression observations, not
independent evidence of drawing correctness.

`tests/data/drawing-inventory-baseline.json` binds those observations to input
hashes. Tests cover full type IDs, inactive ordinals, duplicate/missing owners,
Templates, source ranges, malformed/truncated trailers, unknown codecs,
concatenated frames and aggregate budgets. The `drawing` fuzz target exercises
containers, paired Meta/B inputs and raw record framing; the normal fuzz runner
includes it. Fuzz smoke success is bounded testing, not exhaustive safety proof.

## Provisional sheet, coordinate and text fields

`observations` are selected by segment kind, major31 admission, and the **full type
GUID**, then read at boundaries within that record's typed layout. There is no
payload-wide search in the reader. Every field contains its exact source span and
wire encoding; every observation has `status="unqualified"`. Failed field layouts
emit `drawing.fields_unavailable`, discard that whole observation, and retain the
framed record and its opaque coverage. Unknown types stay opaque.

| Proposed role | SampleBg observations | Decoded wire values |
| --- | ---: | --- |
| Document sheet list | 1 | Counted document label and ordered raw reference list; remaining document state opaque |
| Sheet segment links | 1 | Three counted DC/DL/SM segment names and a label |
| Sheet name | 1 | Counted UTF-16 label `Blatt` |
| Sheet space | 1 | Display/definition references, compact transform and four doubles ending in 42, 29.7 |
| Sheet placement | 3 | Border, title-block and view references, children and compact transform |
| Stored polyline | 19 | Counted f32 XYZ triples, including original Z |
| Stored line / circle / arc | 73 / 2 / 9 | f64 endpoints or center, normal, axis, radius and angular range |
| Attribute lists / booleans / colors | 149 / 2 / 2 | Untagged local attribute references, mask and boolean, RGBA parameters |
| Layer bindings / definitions | 145 / 50 | Meta reference, local cached definition, source width/color/pattern |
| Stroke overrides | 15 | Width, signed dash sequence and retained parameters |
| Image placements | 2 | Size, affine placement, format and embedded stream reference |
| Font directory | 1 | 69 entries with stored ID, family and size/weight candidates |
| Stored text | 55 | Counted UTF-16, six doubles, raw text flags and possible style index |
| Display group | 76 | Raw child references and optional compact transform (74 DL, 2 SM) |

These 607 observations are **not 607 visible objects**. Strings retain NULs,
surrogate pairs and formatting bytes represented as text. Text in DocDC can be a
definition and is deliberately not admitted as displayed text. The inventory
retains raw references and coordinates, without projection, rescaling or assigning
centimetres. Display headers expose object keys and owner references. Meta
sections 7, 8 and 10 retain their exact bytes and spans in `reference_tables`.
The separate experimental interpretation below does not qualify these meanings.

The regression checker independently reconstructs every reported field from the
source bytes (including f32 bit round trips and compact transform masks), and
pins field-value digests. This proves extraction stability, not semantic truth.
Synthetic mutation tests cover other labels/IDs, nonzero Z, Unicode, truncation,
unknown encodings, non-finite values, budgets and ambiguous owners. Controlled
native drawing changes, another major31 drawing and Windows/Autodesk captures
are still needed before publishing a qualified DisplayList or renderer.

## Experimental stored-display scene

```bash
cargo run --locked -p inventor-core --example inspect_drawing_scene -- fixtures/public/SampleBg.idw
```

The example emits `{ "inventory": ..., "sheets": ..., "preview": ..., "images": ... }`. Rust callers can use
`drawing::experimental_scene(&inventory, &limits)` and `drawing::stored_sheets(&inventory, &limits)`. This consumes an inventory
from `inspect`, never opens referenced models, and has a separate work allowance
of `limits.max_records`. The result always has `qualified=false`. A successfully
interpreted space is `experimental_partial`; failure leaves it unavailable with
diagnostics, without retaining a partially traversed space.

The observed major31 grammar joins SM display references through Meta sections
10 → 8 → 7, checks matching context GUIDs, selects a unique registry segment GUID,
and resolves the full display object key (flags and ID). It requires reciprocal,
unique local group ownership and rejects dangling links, cycles, duplicate
instances, invalid affine matrices and traversal depth above 128. It composes
parent × local transforms using column vectors. The experimental branch-0 rule
inherits the parent when a local matrix is absent; raw inventory stays unchanged.

Output items retain XYZ, record spans, placement/group paths and applied matrices.
Lines/polylines are transformed points; circles/arcs remain analytic as center and
two transformed radius vectors plus angular limits. Text retains its transformed
position/direction and an optional directory font matched by stored ID, not row
index. Font height, weight and layout remain candidates. Missing font mappings
remain null. Unsupported reachable nodes and decoded primitives without a stored
sheet binding are listed in `omitted`; opaque records outside this traversal
remain in the inventory and are not counted as visible omissions.

On the pinned SampleBg regression this produces **91 lines/polylines, 11 curves,
53 texts and 2 images** through four stored DL bindings and two SM image placements.
The extra component-view rectangle is excluded by its attached mask-4 false
attribute; the rule does not depend on its ordinal, dimensions or coordinates.
Hidden attributes propagate to descendants. The two text records without a sheet
binding remain omitted, rather than being positioned from their labels.

## Experimental appearance and embedded images

`DisplayItem.style` retains inherited layer width, explicit stroke overrides,
signed dash sequences converted to SVG on/off lengths, available RGBA values,
source spans and `unresolved` reasons. Duplicate/dangling attribute references,
ambiguous layer object keys and invalid numeric values reject the affected space.
Opaque attribute kinds remain explicit. Unknown layer dash patterns are null;
no dash pattern is synthesized from a localized layer name. The diagnostic viewer
uses black when automatic layer color is unresolved, and a diagnostic width when
no width is decoded. Layer widths are in source units, not claimed millimetres.

Some layer references name an earlier revision. Only for the App layer cache, the
experimental resolver permits differing revision/context GUIDs when the segment
GUID and complete object key resolve uniquely inside the same file. It retains
`layer_revision_binding_unverified` plus table/definition spans. This does not
relax context checks for sheet-to-display geometry bindings and does not open
external resources. Historical equivalence of the cached layer remains unverified.

Text retains raw alignment flags, transformed baseline and up vectors, height,
optional positive width factor and raw font flags. The diagnostic renderer uses
both vectors, so rotation/reflection is preserved, and creates multiline text as
literal SVG text nodes. It does not interpret strings as markup. Baseline flags,
font substitution, multiline spacing and complex formatting remain unqualified;
zero font-width fields remain unresolved instead of making text zero-width.

The example also emits `images`, read by
`drawing::read_embedded_images(bytes, &scene, &limits)`. Only numeric references
from emitted image placements can select the exact internal path
`/RSeStorage/RefdFiles/RefdFile_N`. The loader checks the input SHA-256 against the
scene, deduplicates references, and enforces file/stream/aggregate byte, work and
pixel limits. It never uses an external filename. PNG chunk bounds/CRC and
baseline JPEG marker/SOF/SOS bounds are checked; this is not a pixel decoder.
Each result retains original bytes, MIME type, dimensions, digest and source
span, or an unavailable diagnostic without bytes. The two native assets were
also decoded independently with Pillow and loaded in Chromium. Display placement
uses the stored height/width and affine transform with an experimental top-left
anchor; the original image bytes are unchanged.

The current example excludes one explicitly hidden primitive and two unbound
texts. General clipping and drawing-order semantics, complex fonts/annotations,
active state, sheet order, units and other profiles still require independent
inputs and Autodesk comparisons. The standalone diagnostic HTML remains separate
as a research artifact; the [experimental API and Viewer](drawing.md) use the same native scene. Curve sampling, default colors
and fallback fonts are display approximations. The saved thumbnail is visual
context, not a current-state Autodesk oracle.
