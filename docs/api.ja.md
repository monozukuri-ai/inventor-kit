# API ガイド

[English](api.md) | 日本語

## 文書情報

```python
import inventor_kit as ik

doc = ik.inspect_file("part.ipt")
info = doc.metadata
print(info.identification.kind, info.stages)
for prop in info.find_properties(semantic_name="part_number"):
    print(prop.value, prop.source, prop.state_binding)
for diagnostic in info.diagnostics:
    print(diagnostic.code, diagnostic.severity, diagnostic.source)
```

`inspect(bytes, source_id="...")` も利用できます。`read` / `read_file` の結果にも
同じ `metadata` が付きます。`inspect` は通常は形状を解析せず、cq-acis / CadQuery を
import しません。対応するサムネイルは `info.thumbnails` に PNG bytes・寸法・出典を返します。

属性は FMTID/PID、型、コードページ、元バイト列、値、出典を保持します。
文書本体と `MemberDocs` の値を分け、格納場所が異なる候補をすべて返します。
Model State ごとの有効な上書き値や全状態共通値は確定しません。

## 保存候補と形状

```python
inventory = ik.inspect_file("part.ipt", include_candidates=True)
for candidate in inventory.geometry.candidates:
    print(candidate.id, candidate.table_status, candidate.state_binding)

# 一覧の候補 ID を確認した後に明示指定できます。
# doc = ik.read_file("part.ipt", candidate_id=chosen_id)
doc = ik.read_file("part.ipt", require_current_state=True)
```

候補 ID は入力全体の SHA-256 に結び付きます。別ファイルの ID や古い ID では
別候補へフォールバックしません。明示選択も現在状態の証明にはなりません。
`require_current_state=True` は、状態未検証のモデルを返さず診断を残します。

壊れた CFB や入力上限違反は `ValueError`、未対応・曖昧な構造は
`geometry_unavailable` と診断を返します。`decoded_subset` は保存済み B-rep を
共通モデルに取り込めたことを表し、形状変換成功やファイル全体の解釈成功を意味しません。
`doc.model` があっても `doc.to_cadquery()` は未対応曲線・曲面・トリムで停止します。

`doc.kernel_bytes` は選択したカーネルの元データです。エンティティの `SourceSpan` は
展開後 B ストリーム内の位置で、IPT の位置ではありません。
`doc.summary["carrier"]["kernel_offset"]` を引くと `kernel_bytes` 内の位置になります。
形状座標は Inventor の cm から mm に変換し、SAB ヘッダー倍率は `header_scale` に別途保持します。

## アセンブリ

```python
saved = ik.inspect_assembly_file("assembly.iam")
assembly = ik.read_assembly_file("assembly.iam", search_roots=["parts"])
for instance in assembly.instances:
    print(instance.path, instance.resolution, instance.world_transform_mm)

converted = assembly.to_cadquery(allow_unverified_state=True, allow_partial=True)
print(converted.omissions, converted.reference_issues)
report = converted.export_step("assembly.step", allow_partial=True)
print(report["roundtrip"]["status"])
```

`inspect_assembly` とファイル版は、他ファイルや Python の形状モジュールを読み込みません。
部品定義と occurrence を分け、配置行列は mm・行優先で列ベクトルに作用します。
検索は入力 IAM の親ディレクトリと明示ルート内に限定し、ネット取得は行いません。
文書 ID 不一致、同名衝突、欠落、循環、配置未解決を保持し、未知の配置に単位行列を補いません。

`allow_partial=False` が既定です。不完全な変換では部分結果付き
`AssemblyConversionError` を返します。`allow_unverified_state=True` は保存配置の
利用を許可する指定であり、現在状態の検証にはなりません。
`stored_occurrences` は未解釈の UFRx プロパティも型・tag・元位置付きで保持します。

STEP は既存ファイルを上書きせず、付属 JSON に入力 hash と不足情報を保存します。
XDE 再読込で階層・名前・配置・形状量・部品単位の不透明 RGB を照合します。
色の対象は CadQuery に設定した値で、Inventor の native 色は未解析です。

## 文書単位の上限

`Limits` は `read` / `inspect` とファイル版、アセンブリの検査・解決 API に渡せます。
Python と Rust の両方が 0 以上の整数であること、既定値以下であることを検証します。

| フィールド | 既定の最大値 |
| --- | ---: |
| `max_file_bytes` | 128 MiB |
| `max_stream_bytes` | 64 MiB |
| `max_inflated_bytes` | 64 MiB |
| `max_total_inflated_bytes` | 128 MiB / 文書 |
| `max_records` | 500,000 |
| `max_streams` | 65,536 |
| `max_property_bytes` | 16 MiB |
| `max_property_items` | 100,000 |
| `max_property_depth` | 16 |
| `max_candidates` | 256 |

ファイル版は読み込み前後にサイズを確認します。呼び出し元が作成済みの `bytes` は
パーサに渡す前からメモリを使用しています。解析途中の資源不足は既存の部分結果・診断契約に従います。

アセンブリの後続部品変換にも同じ `Limits` を伝えます。グラフ全体の探索量は
`FileSystemResolver` の `max_documents`、`max_instances`、`max_depth`、
`max_total_file_bytes`、`max_directory_entries` で別途制限します。
これらはプロセス RSS・実行時間や OCCT の計算量を保証する値ではありません。
