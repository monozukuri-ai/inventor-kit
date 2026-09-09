# 形式資料と実装境界

IPT / IAM / IDW / IPN は CFB/OLE 文書です。Inventor 固有の RSe コンテナと参照を
このリポジトリで解析し、識別できた SAB/ASM を共通 ACIS コアへ渡します。
IPT 全体を SAT/SAB として扱うことはできません。

## CFB / RSe / カーネル

- [cadmpeg 固定リビジョン](https://github.com/cadmpeg/cadmpeg/tree/faa73bfdaa29a7b4fb5c6998c1bdedfec8fac9d7):
  RSe registry/meta、レコード末尾、型付き候補、UFRx/AmDc/AmGraphics のレイアウト参考。
  実装は独立した限定 reader で、全機能の移植ではありません。
- [cq-acis](https://github.com/monozukuri-ai/cq-acis): 共通 Rust モデル、SAB/ASM、Python/CadQuery 変換。
- [ezdxf](https://github.com/mozman/ezdxf): SAB タグとレコード境界の独立比較。

対応版と未対応データの扱いは [対応範囲](support.md)を参照してください。
InventorLoader の GPL コードは取り込んでいません。

## 文書属性とサムネイル

MS-OLEPS の little endian version 0 / 1、1 / 2 セクション、対応する scalar / vector /
array / VARIANT と PID 辞書を扱います。次元・下限・格納順・コードページ・元バイト列を保持し、
未知の型を隣接型として解釈しません。間接 stream/storage オブジェクトは未対応です。

- [TypedPropertyValue](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-oleps/f122b9d7-e5cf-4484-8466-83f6fd94b3cc)
- [CodePageString](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-oleps/a4c32611-5b79-4965-8f50-50639c138e16)
- [UnicodeString](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-oleps/9660cb24-953a-4e60-adf2-37cc0e779d19)
- [ArrayHeader](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-oleps/e74a8869-440b-43a4-985c-70b850b8aeed)
- [ClipboardData](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-oleps/e5175413-9dad-4a8d-bd1f-058eb301fdc9)
- [PNG 仕様](https://www.w3.org/TR/png-3/)

意味名は確認した FMTID/PID の組に限定します。Design Tracking PID 5 は
`part_number`、標準 SummaryInformation の PID 2/3/4/6 は title/subject/author/comments です。
名前の類似だけでは未知の属性を対応付けません。
[Autodesk の識別子説明](https://blog.autodesk.io/inventor-api-training-lesson-2/)も参照しています。

サムネイルは確認した私有 FMTID/PID と ClipboardData の固定レイアウトに限定し、
PNG のチャンク境界・CRC・IHDR・寸法・終端を検証します。任意位置のシグネチャ走査ではありません。

帰属とライセンスは [THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md)と同梱ライセンスを参照してください。
