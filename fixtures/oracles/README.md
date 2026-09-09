# Independent comparison captures

Store captures as `<input SHA-256>.json`, conforming to
[`vendor-oracle-v1.schema.json`](../../schemas/vendor-oracle-v1.schema.json).
No Autodesk capture has been collected or qualified for M1. The example in
`tests/data/oracle_synthetic.json` is synthetic and is never vendor evidence.
`synthetic/` contains a Cylinder fixture comparison used only to exercise the
report pipeline. Its values and placeholder provider identifiers are artificial.
Reproduce that check with:

```sh
python scripts/validate_public_samples.py --split regression --oracle-dir fixtures/oracles/synthetic --output internal/reports/synthetic-oracle.json
```

On Windows with Autodesk Apprentice and the Python `oracle` extra installed:

```sh
python scripts/capture_vendor_oracle.py --input part.ipt --output fixtures/oracles/INPUT_SHA256.json
```

The collector opens a temporary copy and never calls Save/Update. It verifies
the original/copy hashes after closing. Current implementation targets stored
factory documents; it does not activate Model States. Referenced files are not
copied, so reference-resolution status is specific to the capture environment.
SurfaceBody mass properties require the provider API introduced in 2023; missing
APIs produce an unavailable result with a reason. Database centimetres are
converted to millimetres. Enclosing boxes are not treated as exact bounds.

```sh
python scripts/capture_vendor_oracle.py --validate fixtures/oracles/INPUT_SHA256.json
python scripts/validate_public_samples.py --oracle-dir fixtures/oracles
```

The comparison checks the input hash and reports metric mismatches as failures.
Matching volume/area/solid count and optional precise bounds do not prove full
geometric equality or selected-state ownership. Native SurfaceBody IDs/counts
and ACIS body IDs/counts are recorded in their respective domains. Missing
captures remain `not_collected`; synthetic comparisons remain explicitly marked.

Collector implementation references:
[Apprentice document API](https://help.autodesk.com/cloudhelp/2024/ENU/Inventor-API/files/ApprenticeServerDocument.htm),
[body mass properties](https://help.autodesk.com/cloudhelp/2023/ENU/Inventor-API/files/SurfaceBodyMassProperties.htm),
[database units](https://help.autodesk.com/cloudhelp/2021/ENU/Inventor-API/files/UOM_Overview.htm),
[Model State limitations](https://help.autodesk.com/cloudhelp/2025/ENU/Inventor-API/files/ModelStates_Overview.htm).
