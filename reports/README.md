# Public validation summary

English | [日本語](README.ja.md)

[validation-summary.json](validation-summary.json) publishes verified aggregates
only. `environment` identifies the execution category, `recorded_on` records the
date, and `scope` describes coverage. Counts from separate stages do not establish
support for Inventor as a whole.

Keep the underlying detailed reports in `internal/reports/`.
`scripts/summarize_validation.py` outputs only explicitly selected counts and
statuses. It does not copy paths, detailed diagnostics, machine or host information,
raw properties, or model data. Public CI artifacts use the same summary format.

See the [validation guide](../docs/validation.md) for comparison methods and limits.
