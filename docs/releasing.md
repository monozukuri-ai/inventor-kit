# Release guide

English | [日本語](releasing.ja.md)

## Dependency contract

Rust requires `acis-core=0.3.11` and `acis-py-bridge=0.3.11`. Python requires
`cq-acis>=0.3.11,<0.4` and shared model API 2. The minimum Python version is 3.11,
matching cq-acis. Dependencies must be published, and Cargo.lock must retain
registry sources and checksums for both bridge and core. Results obtained with
local path patches or unpublished wheels do not satisfy this requirement.
This guide does not assert that a release has been published or that CI has passed.

The writer needs the semantic authoring API and JSON-spec fixes in cq-acis 0.3.11.
Publish and verify acis-core, then its matching bridge and Python distribution,
before resolving this project's Cargo and uv locks from public registries. A
staged dependency build is useful for preparation but cannot pass the publish gate.

## Procedure

Version 0.3.0 introduces the [license transition](license.md). Preserve the
earlier MIT notice and third-party conditions. The first commit introducing
the new LICENSE records the source transition; retain its full commit ID and
publication date in the release notes, together with the preceding public MIT
revision. Do not rewrite existing tags or distributions.

Before publishing the first transition release, record the company's authority
to grant the relevant rights and check the named commercial contact. A commercial
information page is not an executed customer agreement. Obtain explicit CLA
acceptance before merging new external contributions.

When changing legal files, run `python scripts/sync_license_notices.py` followed
by the normal viewer build. When Cargo dependencies change, first run
`cargo fetch --locked` and `python scripts/sync_license_notices.py --refresh-rust`.
Review the notice catalog and composite SPDX expression for the actual payload.
The catalog explicitly excludes the UEFI-only dependency from supported targets.

Pass `python scripts/check_license.py` before building. Inspect
`cargo package -p inventor-core --list` and the resulting `.crate` as well as
the Python archives; the self-contained crate LICENSE must match the generated
notice. License-File declarations must point to the actual matching texts in
the wheel's `.dist-info/licenses/` directory or the sdist root. A notice elsewhere
in the archive is insufficient. Retain commercial, legacy MIT and third-party
notices in every distribution and in the standalone viewer assets.

1. In a clean checkout using published dependencies only, pass
   `scripts/check_dependencies.py`, normal CI, corpus checks, and holdout
   validation. Do not carry over `.cargo/config.toml`.
2. Run `python scripts/bump_version.py X.Y.Z` as described below. Rebuild Viewer
   assets only when their inputs change. Run `scripts/check_release.py --tag v<version>`,
   `scripts/check_license.py` and `scripts/check_viewer_assets.py`.
3. Manually run the `Python distributions` workflow. It produces five wheel
   variants, an sdist, and viewer qualification reports as artifacts. The `qualify`
   job must pass even for a manual run. Manual runs do not publish to PyPI.
4. Download each artifact and verify its hash, licenses, contents, and actual
   installation results. `distribution-*` artifacts contain packages;
   `viewer-platform-*` contain only selected installation summaries.
5. Configure or verify the PyPI Trusted Publisher for repository
   `monozukuri-ai/inventor-kit`, workflow `release.yml`, and environment `pypi`.
6. Publish a GitHub Release tagged `v<version>` matching the package version.
   After all gates pass, the workflow publishes through Trusted Publishing.

## Version management

`[workspace.package].version` in the root [Cargo.toml](../Cargo.toml) is the
only release-version definition. Both Rust crates inherit it; Maturin supplies
the Python version through `project.dynamic = ["version"]`. The private Viewer
has no separate release version. Documentation examples do not track the current
release, and third-party dependency versions are managed separately.

From the checkout, with Python 3.11+, Cargo and uv available, replace `X.Y.Z`
with the new stable release version:

```sh
python scripts/bump_version.py X.Y.Z
python scripts/bump_version.py --check --tag vX.Y.Z
```

The update command refreshes both Cargo locks and `uv.lock`, verifies consistency,
and restores its four managed files if any step fails. It rejects unintended
third-party dependency changes. Use `--offline` when the required dependencies
are cached. Alternatively, edit only `Cargo.toml` and run the command without
a version argument. `--check` is read-only and runs in CI and the release tag gate.
No commit, tag or publication is created. Supported versions are stable `X.Y.Z`
without leading zeros; prerelease versions are rejected explicitly.

Dynamic editable packages can omit their version in `uv.lock`. The checker
accepts that form but rejects an explicit stale version. uv's cache keys include
Cargo manifests, locks and native build inputs so it refreshes metadata after a
version change. Rebuild/reinstall local editable extensions after updating.

## CI distribution targets and checks

| Artifact | Build target | Installation checks |
| --- | --- | --- |
| ABI3 wheel | Linux x86_64 / manylinux2014 | Python 3.11 / 3.12 |
| ABI3 wheel | Linux arm64 (aarch64) / manylinux2014 | Python 3.11 / 3.12 |
| ABI3 wheel | Windows x86_64 | Python 3.11 / 3.12 |
| ABI3 wheel | macOS arm64 | Python 3.11 / 3.12 |
| ABI3 wheel | macOS x86_64 | Python 3.11 / 3.12 |
| sdist | Source package | Locked, offline rebuild in a separate directory with a fresh target directory |

Checks cover wheel names, versions, ABI/platform tags, CRCs, RECORD hashes and
sizes, required modules, and licenses. Isolated environments exercise metadata
reading, geometry conversion, and assembly STEP roundtrips, and verify normal
subprocess shutdown. The pinned FTC07 check exercises finite surface references,
saved UV curves, tolerant coedges, and complete conversion into a valid solid
with 258 faces. FTC06 (2021/2024) must produce a valid 146-face primary solid
while rejecting complete-part conversion because two auxiliary open bodies remain.
CTC04 (2021) must produce one closed, valid solid with all 368 faces.
Each platform/interpreter also installs `wheel[viewer]`,
runs `pip check`, starts the local viewer with `--no-browser`, fetches scenes and
mesh buffers for a part, an assembly, a permitted partial part and a permitted partial assembly.
It also checks major31 IDW display with the default and legacy flag, and all four
major23 sheets: resource hashes, shared SVG exports, unverified units and partial status.
All seven cases require normal shutdown with no temporary session data left behind. The sdist is rebuilt
without Node and its wheel is inspected and installed with the viewer extra on
Linux/Python 3.11. Installation checks require wheels for dependencies as well.
`cp310-abi3` denotes the extension's minimum ABI; the package's supported Python
versions are restricted by `Requires-Python: >=3.11`.

The FTC07 volume gate uses adaptive integration at three tolerances, checks its
reported error and convergence, and retains the relative comparison tolerance of
`1e-10`. Default integration values remain in the diagnostic log. Partial FTC06
body export must also preserve both omitted bodies in its checked STEP sidecar.

```sh
maturin build --release --locked --out dist
maturin sdist --out dist
python scripts/check_distribution.py dist/*.whl dist/*.tar.gz
python scripts/smoke_distribution.py --wheel dist/*.whl --viewer --report qualification/wheel.json
python scripts/smoke_distribution.py --sdist dist/*.tar.gz --viewer --report qualification/sdist.json
```

The reusable `verify` job checks Rust, Python, the pinned corpus, viewer asset
regeneration and ASan fuzzing. Release wheel/sdist jobs own archive inspection,
independent rebuilds and cold installation, so `verify` skips that duplicated
work via `skip_distribution_checks`. The Linux x86_64/Python 3.11 wheel job also
runs the full Chromium E2E suite against the actual release wheel; `qualify`
requires that result in its archive-bound report. Normal branch/PR CI keeps local
wheel and sdist qualification, with Chromium E2E on the wheel only. The viewer
checks already include the basic API and writer smoke. Tag pushes do not start
another CI run; the release workflow invokes `verify` for the tagged source.
Host-dependent timing benchmarks remain available through
`scripts/benchmark_parser.py` and are not part of the acceptance workflow.

The four part fixtures each retain offline rendering checks. Shared pan, rotate,
Fit, selection and visibility controls run once on a small asymmetric part.
Browser failures retain a trace and screenshot in `viewer-browser-failure*`.

Browser fixture generation uses the Python invoking `smoke_distribution.py`, which needs
the `validation` extra (including `olefile`). The script sets
`VIEWER_FIXTURE_PYTHON` for this preparation; `VIEWER_PYTHON` still points to the
clean installed `wheel[viewer]` environment for all product execution. Browser
checks use software rendering; they do not qualify physical GPUs or Inventor's
current Model State. The separate platform SVG checks are described below. The [viewer guide](viewer.md)
describes the scenarios and local commands. Missing binary dependencies or runtime
failures fail qualification; do not promote an untested platform to supported.

The `qualify` job checks all six archives and requires eleven reports: five wheel
platforms on each of Python 3.11 / 3.12, plus the Linux sdist rebuild on 3.11. Reports
must match the actual archive SHA-256 and platform, use published dependencies,
and pass every scenario and shutdown check. Missing, duplicate, stale or failed
reports stop the workflow. To recheck downloaded packages and platform reports:

```sh
python scripts/check_release.py --dist dist --viewer-reports qualification
```

Keep only the six packages in `dist/` and the eleven platform reports in
`qualification/`. The separate `validation-report` artifact includes selected
corpus summaries and, in standalone CI, the wheel/sdist viewer summaries. Public summaries contain counts,
platform/interpreter/dependency versions and distribution hashes; document
properties, reference paths, meshes, previews, diagnostics and raw logs remain
excluded. The scene's current-state status remains unverified.

Distributions exclude `internal/`, external CAD files, execution logs, fuzz
corpora, and local Cargo configuration. The sdist includes both README languages,
public `docs/` in English and Japanese, and build inputs. Wheels include the API,
capabilities, and licenses; their package description uses the English README.
Use the source repository for fixture download and validation scripts and fixture
indexes.

The release workflow checks the complete set of five platforms and the sdist.
Every installed-artifact report also requires the writer smoke: a named part
with a hole, a two-occurrence STEP batch with nested rigid placements, the same
batch through the CLI, preserved existing outputs and rejection of unsupported
geometry. Each test creates its own synthetic data. Native acceptance and
per-output `native_validation="not_run"` remain separate from these local checks.
If files for the same version already exist on PyPI, their SHA-256 hashes must
match. Different content or a lookup failure other than HTTP 404 stops the release.
On reruns, `skip-existing` is allowed only after this check.

The distribution workflow additionally requires installed-wheel SVG
checks in Chromium on all five platforms with Python 3.12. It renders five public
fixture sheets and one synthetic font/dash control, checking bounds, image counts
and actual Japanese/diameter-symbol fonts. `drawing-browser-*` artifacts contain
PNGs and browser/version evidence. These checks do not qualify platform-specific
3D browser interaction, Safari/WebKit, native font fidelity or physical printing.
`workflow_dispatch` runs qualification without publishing a release.
