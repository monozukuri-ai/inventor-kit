# User and developer documentation

English | [日本語](README.ja.md)

- [Licensing](license.md): Noncommercial permissions, commercial plans and earlier MIT material.
- [API guide](api.md): Reading files, metadata, saved candidates, assemblies, and limits.
- [Body selection](body-conversion.md): Individual results, explicit partial STEP and source IDs.
- [CLI and batches](cli.md): IPT/IAM jobs, JSON reports, exit codes and timeouts.
- [Local viewer](viewer.md): Saved IPT geometry and IAM placements, previews, properties, and read diagnostics.
- [Experimental IDW display](drawing.md): Saved drawing sheets, Python API, Viewer usage and limitations.
- [Supported scope](support.md): Validated profiles and unsupported or unverified behavior.
- [Validation](validation.md): Comparison methods, holdouts, and interpreting results.
- [Development](development.md): Building and reproducing checks using public sources only.
- [Releasing](releasing.md): Distribution CI, dependency contracts, and publication requirements.
- [Format references](format-reference.md): Source material and attribution.

Public guides use English by default and provide Japanese versions in sibling
`.ja.md` files. Use the language link at the top of each guide to switch languages.
Update both versions when changing documented behavior or commands.

Keep usage, API contracts, support limits and reproducible contributor procedures
in `docs/`. Put implementation plans, format investigations and collection procedures
in `internal/docs/`. Keep all validation summaries, session logs, raw comparisons
and host details together in `internal/reports/`.
Public guides must not depend on files under `internal/`. The Python API and
[capabilities.json](../python/inventor_kit/capabilities.json) define the public contract.
