# Development and public directories

English | [日本語](development.ja.md)

## Build and validate

The library and development scripts require Python 3.11 or later. Use Rust 1.93
to match CI; maturin 1.11.5 is the validation baseline. Normal development and
CI require `acis-core` / `acis-py-bridge` 0.3.2 and `cq-acis>=0.3.2,<0.4`
from public registries. If an earlier development `.cargo/config.toml`
overrides the bridge, move that configuration into `internal/` before building.
`scripts/check_dependencies.py` verifies that core and bridge each resolve to a
single published crate.

```sh
python3.11 -m venv .venv
. .venv/bin/activate
python -m pip install 'maturin[patchelf]==1.11.5'
maturin develop --release --locked --extras validation
python scripts/check_dependencies.py
python scripts/check_public_tree.py
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
python scripts/fetch_public_samples.py
python scripts/fetch_assembly_samples.py
python scripts/run_tests.py
```

On Windows, use `.venv/Scripts/python.exe` in the virtual environment. Fetching
fixtures is an explicit network operation. Once fetched, normal parsing and
validation can run offline. `run_tests.py` checks every manifest entry for
existence, size, and SHA-256, and treats skipped tests as failures.

```sh
python scripts/validate_public_samples.py
python scripts/validate_document_samples.py
python scripts/validate_candidate_samples.py
python scripts/validate_geometry.py
python scripts/validate_components.py
python scripts/validate_topology.py
python scripts/validate_assembly.py
python scripts/benchmark_parser.py
```

Detailed results are written to `internal/reports/latest/` by default. No internal
material is required for the first run. Geometry comparisons use the public
[geometry-baseline.json](../tests/data/geometry-baseline.json); benchmark inputs
are defined in [cases.json](../benchmarks/cases.json).

## Fuzzing

```sh
rustup toolchain install nightly-2026-02-02 --profile minimal
cargo install cargo-fuzz --version 0.13.2 --locked
cargo fetch --manifest-path fuzz/Cargo.toml --locked
python scripts/seed_fuzz_corpus.py
python scripts/run_fuzz.py --seconds 30
```

`fuzz/` has its own lockfile. Seeds use regression fixtures and synthetic inputs,
with holdouts excluded. Two ASan targets exercise CFB documents and RSe/UFRx/SAB
streams. A short smoke run does not establish exhaustive coverage.

## Public directory roles

| Path | Contents |
| --- | --- |
| `python/`, `crates/` | Library implementation and Rust tests |
| `docs/`, `README.md`, `README.ja.md` | English and Japanese API, scope, validation, and distribution guides |
| `scripts/` | Reproducible build, validation, and distribution tools |
| `tests/`, `schemas/`, `fixtures/*.json` | Tests, public contracts, and pinned input sources and hashes |
| `benchmarks/` | Fixed inputs and measurement methods |
| `reports/` | Public summaries containing verified aggregates only |
| `internal/` | Work logs, old plans, exploratory code, host details, and detailed results |

`internal/` is excluded from Git and wheel/sdist packages. External CAD files,
fuzz corpora/artifacts, and local Cargo configuration are also excluded from
public packages. Public code does not require internal files as inputs, except
for comparison results or oracles explicitly supplied by the user. `.gitignore`,
Git archive `export-ignore`, distribution checks, and CI checks of public links
maintain this boundary.

Successful checks with local development dependencies do not establish a
successful build using published dependencies alone. Releases must separately
meet the [distribution requirements](releasing.md).

## Documentation languages

Use English in `README.md` and public guides. Keep Japanese translations in
sibling `.ja.md` files and link both versions at the top of each page. When
changing commands, dependency versions, API behavior, or support and validation
claims, update both languages in the same change. Japanese guides should link
to Japanese versions where available; API identifiers and commands stay the same.
Run `python scripts/check_public_tree.py` after editing links.
