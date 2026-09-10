# Format references and implementation boundaries

English | [日本語](format-reference.ja.md)

IPT / IAM / IDW / IPN are CFB/OLE documents. This repository parses Inventor's
RSe containers and references, then passes identified SAB/ASM payloads to the
shared ACIS core. An entire IPT file cannot be treated as SAT/SAB data.

## CFB / RSe / kernel

- [Pinned cadmpeg revision](https://github.com/cadmpeg/cadmpeg/tree/faa73bfdaa29a7b4fb5c6998c1bdedfec8fac9d7):
  Layout references for RSe registry/meta, record trailers, typed candidates,
  and UFRx/AmDc/AmGraphics. The implementation is an independent reader with
  limited scope, rather than a complete port.
- [cq-acis](https://github.com/monozukuri-ai/cq-acis): Shared Rust models,
  SAB/ASM parsing, and Python/CadQuery conversion.
- [ezdxf](https://github.com/mozman/ezdxf): Independent comparison of SAB tags
  and record boundaries.

See [supported scope](support.md) for validated versions and treatment of
unsupported data. GPL code from InventorLoader is not incorporated.

## Document properties and thumbnails

The property reader handles little-endian MS-OLEPS versions 0 / 1, one or two
sections, supported scalar / vector / array / VARIANT values, and PID
dictionaries. It retains dimensions, lower bounds, storage order, code pages,
and original bytes. Unknown types are not interpreted as adjacent types.
Indirect stream/storage objects are unsupported.

- [TypedPropertyValue](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-oleps/f122b9d7-e5cf-4484-8466-83f6fd94b3cc)
- [CodePageString](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-oleps/a4c32611-5b79-4965-8f50-50639c138e16)
- [UnicodeString](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-oleps/9660cb24-953a-4e60-adf2-37cc0e779d19)
- [ArrayHeader](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-oleps/e74a8869-440b-43a4-985c-70b850b8aeed)
- [ClipboardData](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-oleps/e5175413-9dad-4a8d-bd1f-058eb301fdc9)
- [PNG specification](https://www.w3.org/TR/png-3/)

Semantic names are limited to verified FMTID/PID pairs. Design Tracking PID 5
maps to `part_number`; standard SummaryInformation PIDs 2/3/4/6 map to
title/subject/author/comments. Unknown properties are not mapped based on name
similarity alone. The reader also refers to
[Autodesk's identifier documentation](https://blog.autodesk.io/inventor-api-training-lesson-2/).

Thumbnail support is limited to verified private FMTID/PID pairs and a fixed
ClipboardData layout. PNG chunk boundaries, CRCs, IHDR, dimensions, and termination
are validated. The reader does not scan arbitrary offsets for signatures.

See [THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md) and the bundled license
files for attribution and licensing.
