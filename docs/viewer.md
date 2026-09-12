# Local part and assembly viewer

English | [日本語](viewer.ja.md)

The optional viewer displays supported saved IPT geometry and IAM placements, document properties,
saved previews, geometry candidates, and read diagnostics. It runs locally using
the same bounded parser and CadQuery conversion as the Python API.

```sh
python -m pip install 'inventor-kit[viewer]'
python -m inventor_kit.viewer part.ipt
```

The command prints a loopback URL and opens a browser. Keep the process running
while viewing; press Ctrl-C in the terminal to stop it. Closing the browser tab
does not stop the process. Node.js, Inventor, and runtime network access are not
required. Windows also accepts Ctrl-Break for normal shutdown.

The distribution workflow checks the viewer extra on Python 3.11 and 3.12 for
Linux x86_64, Windows x86_64, and macOS arm64 / x86_64. Each fresh installation
must serve IPT, saved IAM, and explicitly permitted partial IAM scenes, deliver
mesh buffers, exit normally, and remove temporary session data. Full browser
checks run on Linux Chromium using software rendering. Windows/macOS browser
rendering and physical GPU performance are not covered by the server checks.
See the [release guide](releasing.md) for required results and artifacts.

For an unreleased revision, download its platform wheel from a successful
`Python distributions` workflow run and install it using
`python -m pip install './inventor_kit-<version>-<abi>-<platform>.whl[viewer]'`
with the actual filename. The PyPI command above installs the published release,
which may not yet include that revision's viewer changes.

```sh
python -m inventor_kit.viewer part.ipt --no-browser --port 0
python -m inventor_kit.viewer part.ipt --quality fine
python -m inventor_kit.viewer part.ipt --candidate-id CANDIDATE_ID
python -m inventor_kit.viewer part.ipt --require-current-state
python -m inventor_kit.viewer drawing.idw --metadata-only
python -m inventor_kit.viewer assembly.iam
python -m inventor_kit.viewer assembly.iam --allow-unverified-state
python -m inventor_kit.viewer assembly.iam --search-root parts --allow-unverified-state --allow-partial
```

`--port 0` selects an available port. `--no-browser` prints the URL without opening
it. `--metadata-only` also works without the viewer extra and does not import the
Python geometry modules. It cannot be combined with the candidate/current-state
options. The candidate ID must belong to the exact input; see the [API guide](api.md).
Candidate selection is made at startup. To view another input or candidate,
restart the command with that path or ID. Assembly options cannot be combined with
`--metadata-only` or the IPT candidate/current-state options.

IAM opens its reference graph and occurrence tree by default, without geometry
imports. To display saved placements, pass `--allow-unverified-state`. This
acknowledges that current Model State, visibility and substitute selection are
unverified; it does not verify them. If any required geometry is unavailable,
the tree and reasons remain visible and no partial geometry is displayed unless
`--allow-partial` is also given. This applies to tessellation failures as well as
the existing converter's omissions. `--allow-partial` requires
`--allow-unverified-state`.

Repeat `--search-root` for additional reference directories. The existing offline
resolver searches the root document's directory and these explicit roots, without
recursive search. Identity mismatches, name collisions, cycles, missing files and
unsupported placements retain their original reasons. Unplaced components stay
in the tree without a mesh; unknown transforms are never replaced with an origin
placement. Changes to a resolved part, assembly or source alias reject pending
geometry. See the [assembly API](api.md) for the resolver's support boundary.

Use the mouse to orbit, pan, and zoom. The toolbar provides Fit, Isometric, Front,
Top, and Edges. Select a body in the left panel, or double-click it in the 3D view,
to show its selection box. IAM uses an occurrence hierarchy, including omitted
components and assembly groups. The checkboxes control visibility; group controls
affect descendants. **Isolate** shows the selected occurrence or group alone, and
**Show all** restores all available geometry. Repeated occurrences remain
independently selectable while sharing definition meshes and buffer downloads.
These controls change only the viewer's display, not saved visibility or state. Display colors
are assigned by the viewer; Inventor appearance is not decoded.

The notice **Current Model State is unverified** applies even when a mesh renders
successfully. Saved previews can show other stored states and are labelled
separately. Geometry conversion failures retain available properties, previews,
candidates, and the original diagnostic. IDW and IPN currently show document
information only; drawing rendering and presentation animation are unsupported.
See [supported scope](support.md).

`--quality` accepts `draft`, `normal` (default), or `fine`. Their absolute OCCT
linear deflection settings are 0.3 / 0.1 / 0.03 mm and angular deflections are
0.3 / 0.1 / 0.05 radians. These are display settings, not certified measurement
errors. Every B-rep face must have triangles; an omitted face rejects the part's
display mesh. B-rep edges are discretized separately. The viewer does not add
support for curves or surfaces that the CadQuery converter rejects.

Conversion runs in a spawned process with a default 120-second deadline, changed
with `--timeout`. The parser's [document limits](api.md#per-document-limits) still
apply. Display output is limited to two million triangles, 128 MiB of mesh buffers,
16 MiB of scene JSON, and 64 saved previews / 32 MiB of PNG bytes. The triangle
limit also applies after expanding repeated occurrences. The existing resolver
defaults apply: 256 documents, 10,000 instances, depth 32, 512 MiB of input files
and 100,000 directory entries. These ceilings
do not bound OCCT's intermediate memory allocations or promise a process RSS limit.
Worker timeout or abnormal exit discards pending geometry and retains already
published document information and any occurrence inventory. A normal viewer startup is not a complete-model
claim. Read failures are shown in the UI; invalid arguments and startup failures
produce a nonzero CLI exit.

The server binds only to `127.0.0.1` and serves packaged assets and the active
session's generated resources. It provides no upload or arbitrary file API.
Temporary session data is removed on normal shutdown. Installation and initial
fixture downloads need network access; viewing an installed package does not.

For development, the frontend lives in `viewer/` and its generated assets are
checked in under `python/inventor_kit/viewer/static/`. Install the Python viewer
and validation extras in the development environment before running integration
tests. Node 22.18.0 / npm 10.9.3 are the asset build baseline.

```sh
npm ci --prefix viewer
npm run build --prefix viewer
python scripts/check_viewer_assets.py
python scripts/run_viewer_tests.py
cd viewer
npx playwright install chromium
npm test
```

The viewer's Python integration tests live in `viewer_tests/`, separate from the
base suite; missing required dependencies/fixtures and skips fail that runner.
Browser tests block external requests and check real parts, unsupported geometry,
current-state refusal, document-only viewing, and corrupt input. Screenshots are
local test artifacts, not Inventor comparison evidence. The mesh tests also use
an analytic tube to check its hole, signed volume and triangle orientation. IAM
tests cover the native Subassembly and the five displayable parts / two omissions
in SampleBg. Synthetic tests check repeated definitions, noncommuting parent/child
turns, intrinsic shape locations, unknown placements, source changes, partial
tessellation failure and independent selection. The held-out IAM remains rejected
by its unsupported native profile.

The internal scene uses row-major 4×4 matrices acting on column vectors in mm.
Definition meshes keep their local geometry; each hierarchy edge applies its local
placement once. Composed transforms are checked against the saved world matrices
and the converter's actual placement chain. The frontend converts local rotations
to renderer quaternions. Bounds frame the camera and are not exact CAD dimensions.

```sh
python scripts/smoke_distribution.py --wheel dist/inventor_kit-0.2.0-cp310-abi3-manylinux_2_34_x86_64.whl --viewer --browser
python scripts/smoke_distribution.py --sdist dist/inventor_kit-0.2.0.tar.gz --viewer --browser
```

Use the actual built wheel filename for your environment. Omit `--browser` for
the Windows/macOS server checks; optionally select the interpreter with
`--python`. These checks install outside the checkout and verify local serving
and cleanup. `--browser` additionally runs the Linux Chromium tests using that
installed interpreter. `--report <file.json>` writes selected counts, dependency
versions, platform and Python versions and the input distribution's SHA-256 only after
all requested checks and interpreter shutdown pass. It excludes document
properties, reference paths, mesh data, previews and detailed diagnostics. The sdist includes built assets and their
source/lockfile; rebuilding the Python wheel does not require Node. See
[releasing](releasing.md) for distribution gates.
