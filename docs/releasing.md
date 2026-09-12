# Release guide

English | [日本語](releasing.ja.md)

## Dependency contract

Rust requires `acis-core=0.3.3` and `acis-py-bridge=0.3.3`. Python requires
`cq-acis>=0.3.3,<0.4` and shared model API 2. The minimum Python version is 3.11,
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
   variants, an sdist, and viewer qualification reports as artifacts. The `qualify`
   job must pass even for a manual run. Manual runs do not publish to PyPI.
4. Download each artifact and verify its hash, licenses, contents, and actual
   installation results. `distribution-*` artifacts contain packages;
   `viewer-platform-*` contain only selected installation summaries.
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
subprocess shutdown. The pinned FTC07 check exercises finite surface references,
saved UV curves, and a tolerant coedge, while requiring rejection of the
unqualified complete part. Each platform/interpreter also installs `wheel[viewer]`,
runs `pip check`, starts the local viewer with `--no-browser`, fetches scenes and
mesh buffers for a part, an assembly and a permitted partial assembly, and requires
normal shutdown with no temporary session data left behind. The sdist is rebuilt
without Node and its wheel is inspected and installed with the viewer extra on
Linux/Python 3.11. Installation checks require wheels for dependencies as well.
`cp310-abi3` denotes the extension's minimum ABI; the package's supported Python
versions are restricted by `Requires-Python: >=3.11`.

```sh
maturin build --release --locked --out dist
maturin sdist --out dist
python scripts/check_distribution.py dist/*.whl dist/*.tar.gz
python scripts/smoke_distribution.py --wheel dist/*.whl --viewer --report qualification/wheel.json
python scripts/smoke_distribution.py --sdist dist/*.tar.gz --viewer --report qualification/sdist.json
```

The prerequisite Linux CI job must also pass Chromium E2E against both an
installed wheel and a separately rebuilt sdist. It checks bundled assets against
`npm ci` / asset regeneration using the pinned lockfile and Node baseline. Browser
checks use software rendering; they do not qualify Windows/macOS browser rendering,
physical GPUs, or Inventor's current Model State. The [viewer guide](viewer.md)
describes the scenarios and local commands. Missing binary dependencies or runtime
failures fail qualification; do not promote an untested platform to supported.

The `qualify` job checks all five archives and requires nine reports: four wheel
platforms on each of Python 3.11 / 3.12, plus the Linux sdist rebuild on 3.11. Reports
must match the actual archive SHA-256 and platform, use published dependencies,
and pass every scenario and shutdown check. Missing, duplicate, stale or failed
reports stop the workflow. To recheck downloaded packages and platform reports:

```sh
python scripts/check_release.py --dist dist --viewer-reports qualification
```

Keep only the five packages in `dist/` and the nine platform reports in
`qualification/`. Linux CI's separate `validation-report` artifact includes
selected corpus and viewer browser summaries. Public summaries contain counts,
platform/interpreter/dependency versions and distribution hashes; document
properties, reference paths, meshes, previews, diagnostics and raw logs remain
excluded. The scene's current-state status remains unverified.

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
