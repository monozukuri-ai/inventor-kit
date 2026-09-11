# User and developer documentation

English | [日本語](README.ja.md)

- [API guide](api.md): Reading files, metadata, saved candidates, assemblies, and limits.
- [Local viewer](viewer.md): Saved IPT geometry and IAM placements, previews, properties, and read diagnostics.
- [Supported scope](support.md): Validated profiles and unsupported or unverified behavior.
- [Validation](validation.md): Comparison methods, holdouts, and interpreting results.
- [Development](development.md): Building and reproducing checks using public sources only.
- [Releasing](releasing.md): Distribution CI, dependency contracts, and publication requirements.
- [Format references](format-reference.md): Source material and attribution.

Public guides use English by default and provide Japanese versions in sibling
`.ja.md` files. Use the language link at the top of each guide to switch languages.
Update both versions when changing documented behavior or commands.

Work logs, milestone plans, host-specific measurements, and exploratory code are
excluded from public documentation. The Python API and
[capabilities.json](../python/inventor_kit/capabilities.json) define the public contract.
