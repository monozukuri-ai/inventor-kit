# Command line and batch conversion

English | [日本語](cli.ja.md)

The existing single-input inspection syntax remains available. Geometry conversion
and structured reports in this guide require v0.4.0.

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
format. `--report`, `--jsonl`, and conversion modes use the
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
