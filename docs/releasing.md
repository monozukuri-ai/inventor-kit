# Release guide

English | [日本語](releasing.ja.md)

## Dependency contract

Rust requires `acis-core=0.3.2` and `acis-py-bridge=0.3.2`. Python requires
`cq-acis>=0.3.2,<0.4` and shared model API 2. The minimum Python version is 3.11,
matching cq-acis. Dependencies must be published, and Cargo.lock must retain
registry sources and checksums for both bridge and core. Results obtained with
local path patches or unpublished wheels do not satisfy this requirement.
This guide does not assert that a release has been published or that CI has passed.

## Procedure

1. In a clean checkout using published dependencies only, pass
   `scripts/check_dependencies.py`, normal CI, corpus checks, and holdout
   validation. Do not carry over `.cargo/config.toml`.
2. Align the Python package version with both Rust package versions.
   `scripts/check_release.py` validates the versions.
3. Manually run the `Python distributions` workflow. It produces four wheel
   variants and an sdist as artifacts. Manual runs do not publish to PyPI.
4. Download each artifact and verify its hash, licenses, contents, and actual
   installation results.
5. Configure or verify the PyPI Trusted Publisher for repository
   `monozukuri-ai/inventor-kit`, workflow `release.yml`, and environment `pypi`.
6. Publish a GitHub Release tagged `v<version>` matching the package version.
   After all gates pass, the workflow publishes through Trusted Publishing.

## CI distribution targets and checks

| Artifact | Build target | Installation checks |
| --- | --- | --- |
| ABI3 wheel | Linux x86_64 / manylinux2014 | Python 3.11 / 3.12 |
| ABI3 wheel | Windows x86_64 | Python 3.11 / 3.12 |
| ABI3 wheel | macOS arm64 | Python 3.11 / 3.12 |
| ABI3 wheel | macOS x86_64 | Python 3.11 / 3.12 |
| sdist | Source package | Locked, offline rebuild in a separate directory with a fresh target directory |

Checks cover wheel names, versions, ABI/platform tags, CRCs, RECORD hashes and
sizes, required modules, and licenses. Isolated environments exercise metadata
reading, geometry conversion, and assembly STEP roundtrips, and verify normal
subprocess shutdown. Installation checks require wheels for dependencies as well.
`cp310-abi3` denotes the extension's minimum ABI; the package's supported Python
versions are restricted by `Requires-Python: >=3.11`.

```sh
maturin build --release --locked --out dist
maturin sdist --out dist
python scripts/check_distribution.py dist/*.whl dist/*.tar.gz
python scripts/smoke_distribution.py --wheel dist/*.whl
python scripts/smoke_distribution.py --sdist dist/*.tar.gz
```

Distributions exclude `internal/`, external CAD files, execution logs, fuzz
corpora, and local Cargo configuration. The sdist includes both README languages,
public `docs/` in English and Japanese, and build inputs. Wheels include the API,
capabilities, and licenses; their package description uses the English README.
Use the source repository for fixture download and validation scripts and fixture
indexes.

The release workflow checks the complete set of four platforms and the sdist.
If files for the same version already exist on PyPI, their SHA-256 hashes must
match. Different content or a lookup failure other than HTTP 404 stops the release.
On reruns, `skip-existing` is allowed only after this check.
