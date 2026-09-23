# Command line and batch conversion

English | [日本語](cli.ja.md)

The existing single-input inspection syntax remains available. Geometry conversion
and conversion reports require v0.4.0 or later. IDW reports and SVG export require v0.5.0 or later.

```sh
python -m inventor_kit part.ipt
python -m inventor_kit drawing.idw --metadata-only
python -m inventor_kit part.ipt --list-candidates
python -m inventor_kit part.ipt --list-bodies
python -m inventor_kit part.ipt --convert --report part-report.json
python -m inventor_kit part.ipt --step part.step
python -m inventor_kit assembly.iam --step assembly.step --allow-unverified-state
python -m inventor_kit a.ipt b.ipt --output-dir converted --jsonl --report results.jsonl
```

`--step` and `--output-dir` imply conversion. File contents determine document
kind. IAM uses the existing resolver; repeat `--search-root` for local reference
directories. `--allow-unverified-state` acknowledges saved IAM placements.
`--allow-partial` permits output with recorded omissions for IPT or IAM.
Candidate and body IDs are restricted to a single IPT input; repeat `--body-id`
to select several bodies. See [body selection](body-conversion.md).

`--list-bodies` attempts conversion and reports every body without exporting it.
`--metadata-only` and `--list-candidates` avoid Python geometry imports.
Without new conversion/report flags, a single input retains its existing summary
format. For inspection and geometry conversion, `--report`, `--jsonl`, and conversion modes use the
[conversion report v1 schema](../schemas/conversion-report-v1.schema.json).
Multiple inputs otherwise produce a JSON array.

| Exit code | Result |
| --- | --- |
| 0 | All requested jobs succeeded |
| 2 | Partial geometry or selection; consult omissions |
| 3 | Unsupported input/geometry, with no usable result |
| 1 | Invalid options/input, refused output, timeout, process or export failure |

For a mixed batch, errors take precedence over unsupported and partial results.
Legacy single-input inspection keeps its original zero exit code when an
unsupported profile/state is represented in the summary; use `--report` or
`--jsonl` for the classified exit codes above. The original single-part `--step`
invocation also retains summary stdout unless new report/selection flags are used.
All inputs still receive a result unless the overall output plan is invalid.
Partial reports may be inspected without `--allow-partial`; partial STEP export
requires it. Every result distinguishes geometric success from current state,
which remains unverified. CLI reports add inspection/conversion/export stages.

Each input runs in a separate process with a default 120-second deadline, including
native conversion. Set `--timeout SECONDS` to change it. Native stdout/stderr is
kept out of JSON stdout. Process crashes and timeouts produce classified results;
later inputs continue. STEP files are staged and published only after normal
worker shutdown. This is not an operating-system memory limit.

Outputs and report files are never overwritten. Batch STEP names use input stems;
duplicate names (including case aliases) are rejected before jobs run. Use separate
directories for such inputs. Successful earlier batch outputs remain if a later
input fails. Each STEP has a checked provenance/omission JSON sidecar.

## IDW reports and SVG export

```sh
python -m inventor_kit drawing.idw --list-sheets
python -m inventor_kit drawing.idw --drawing-report --report drawing.json
python -m inventor_kit a.idw b.idw --drawing-report --jsonl --report drawings.jsonl
python -m inventor_kit drawing.idw --drawing-report --drawing-details --sheet-id 'ID_FROM_LIST'
python -m inventor_kit drawing.idw --svg sheet.svg --sheet-id 'ID_FROM_LIST' --allow-partial
```

These modes use the [drawing report v1 schema](../schemas/drawing-report-v1.schema.json)
and do not import Python geometry libraries. `--list-sheets` provides a compact
inventory; `--drawing-report` adds saved views, raw text, styles, sources, image
metadata and omissions. `--drawing-details` also includes every geometry item.
`--sheet-id` accepts an input-bound ID from the inventory, never a sheet name.
Selection and SVG export require one input; multiple IDW reports support JSONL.
An SVG export may omit the ID only when the document has exactly one sheet.

`--svg` writes `sheet.svg` and `sheet.svg.json`. It requires `--allow-partial`,
refuses existing files, and publishes output only after normal worker shutdown.
Images are embedded; no browser, Node.js or font download is needed to export.
The sidecar records the source/SVG hashes, selected sheet, omissions and original
text. The SVG limit is 32 MiB. Per-worker JSON has a 96 MiB limit for detailed reports and a 16 MiB limit
otherwise. Final pretty printing and multi-input aggregation can be larger; use
`--jsonl` for compact output with large geometry inventories. Reports and exports retain unverified source
coordinates and are not qualified for physical-scale printing or measurements.

Supported IDW reports and SVG exports currently exit **2**, including when files
were successfully saved, because support is partial. Unsupported profiles exit
3; invalid IDs, refused partial export, timeout or output errors exit 1. Reporting
alone needs no partial opt-in. Geometry, metadata-only, candidate/body and IAM
selection flags cannot be mixed with drawing modes.
