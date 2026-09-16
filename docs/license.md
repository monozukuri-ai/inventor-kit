# Licensing

English | [日本語](license.ja.md)

Starting with **0.3.0**, inventor-kit offers new project material under the
[PolyForm Noncommercial License 1.0.0](../licenses/PolyForm-Noncommercial-1.0.0.md),
with separate commercial licenses from **UnRobotics Inc.** It is source-available.
The [license notice](../LICENSE) defines the earlier-MIT and third-party boundaries.

## Choosing a license

| Use | Route |
| --- | --- |
| Personal study or hobby without an anticipated commercial application | PolyForm, subject to its terms |
| Educational institutions, public research organizations and other organizations explicitly covered by PolyForm | The permissions in the official terms apply, regardless of funding source |
| Business automation, internal conversion, commercial development | Commercial Internal, when outside PolyForm's permissions |
| Integration into a product distributed to customers | Commercial OEM, including Internal use by the same entity |
| Providing an SDK for other companies to develop products | SDK redistribution addendum |
| Customer-facing hosted conversion or analysis | SaaS addendum |
| A company's pre-purchase feasibility test | Request a separate 30-day evaluation license |

Company size alone does not create a free commercial tier. Internal use is not
automatically noncommercial. Conversely, this guide does not restrict the
noncommercial and organizational permissions in the official PolyForm terms.
A university's permission does not automatically cover a separate company.
The license also grants distribution permissions: passing on a copy and granting
the recipient commercial execution or development rights are separate questions.

See [commercial licensing](../COMMERCIAL-LICENSE.md) for scope, quotation,
contract expiry, OEM end-user rights and support. No online activation is required.

## Earlier versions and source

The MIT grants for 0.1.0, 0.2.0 and previously published MIT source remain in
effect, including source at
[`440f149`](https://github.com/monozukuri-ai/inventor-kit/tree/440f149fe96a4c8e7f207ce621919284f6a1f621).
Their original notice is retained in
[inventor-kit-legacy-MIT.txt](../licenses/inventor-kit-legacy-MIT.txt).
The source transition is recorded by the first commit introducing the new
LICENSE notice; the package transition begins with 0.3.0. Earlier MIT rights
are not revoked by copying that material into a newer package.

Users requiring the old distribution can pin `inventor-kit==0.2.0` and retain
their dependency lockfile. Pinning does not promise future fixes or compatibility.
The old MIT notice does not license all later additions under MIT.

## Third-party material and outputs

[THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md) identifies components,
reference material and their notices. cq-acis and its Rust crates keep their
own MIT licenses. Python dependencies such as CadQuery and OCP are separate
distributions; bundling them in an OEM installer requires complying with their
licenses and those of the native libraries they contain.

The package's composite SPDX expression describes different components and
attribution material, not alternative licenses for every file. It does not mean
that the whole project can be used under MIT or Apache-2.0. License texts are
included in wheel metadata, the sdist, each Rust crate and the viewer bundle.

We do not claim extra ownership or output-based royalties in your converted
CAD files, images or metadata. Input and third-party rights still apply.
Downloading public sample files does not establish redistribution permission.

## Contributions

New external contributions need the explicit, nonexclusive commercial
sublicensing grant in [CLA.md](../CLA.md). See
[CONTRIBUTING.md](../CONTRIBUTING.md) for acceptance and authority records.
This does not transfer contributors' copyright or change earlier MIT grants.
