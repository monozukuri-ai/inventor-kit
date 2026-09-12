# Public development tools

English | [日本語](README.ja.md)

Requires Python 3.11 or later. See the [development guide](../docs/development.md)
for standard reproduction steps. Detailed results default to
`internal/reports/latest/`. The tools can run in a clone without internal material.

| Tool | Purpose |
| --- | --- |
| `fetch_public_samples.py`, `fetch_assembly_samples.py` | Fetch fixtures from pinned sources |
| `check_corpus.py`, `corpus_manifest.py`, `run_tests.py` | Verify hashes and dataset splits; run tests with no skips allowed |
| `validate_public_samples.py` | Validate container, shared model, geometry, and optional oracle stages separately |
| `validate_document_samples.py`, `validate_candidate_samples.py` | Validate metadata, saved candidates, and provenance |
| `validate_geometry.py`, `validate_components.py`, `validate_topology.py` | Check frozen geometry metrics, curves, surfaces, and topology |
| `validate_tolerant_trims.py` | Check the [development tolerant/finite-UV extension](../docs/tolerant-trims.md) against pinned source components |
| `validate_assembly.py` | Regression and holdout checks for IAM references, placements, omissions, and STEP |
| `benchmark_parser.py`, `seed_fuzz_corpus.py`, `run_fuzz.py` | Measure fixed inputs and run bounded ASan fuzzing |
| `capture_vendor_oracle.py`, `capture_model_states.py` | Optional Windows / Autodesk oracle capture |
| `oracle_contract.py`, `compare_state_candidates.py`, `ezdxf_oracle.py` | Oracle contracts and comparisons |
| `check_dependencies.py`, `check_distribution.py`, `smoke_distribution.py`, `check_release.py` | Dependency, distribution, isolated installation, and release gates |
| `check_public_tree.py`, `summarize_validation.py` | Check public links and generate public aggregates |

Store temporary dependency patches, early byte-level exploration, and code used
to investigate development history in `internal/scripts/`. Public CI does not
invoke that code.
