# アセンブリwriter

[English](assembly-writer.md) | 日本語

実験段階のIAM writerは、新規生成したIPTの**1～32配置**を平坦なアセンブリへまとめます。
Inventor・COM・既存IAM・nativeテンプレートは使いません。
限定profileは2026-10-12にInventor 2027.1（build `311270010`）で検証済みです。
APIは作業ツリーへ実装済みですが、リリースは未実施です。

## Python

```python
import inventor_kit as ik

assembly = ik.new_assembly(name="Frame", part_number="ASM-001")
assembly.add_part("parts/rail.ipt", name="Rail:1")
assembly.add_part("parts/plate.ipt", name="Plate:1")
result = assembly.save_bundle("frame_bundle", filename="frame.iam")
print(result.path, result.definition_count, result.occurrence_count)
```

入力は限定した単一solid profileの `new_part(...).saveas(...)` または
`write_step_parts(...)` が生成したIPTです。文書ID・表示名・boundsはIPT本体から読み、
sidecar JSONをidentityの根拠にしません。一般のnative IPTやInventorで再保存したIPTは対象外です。

配置は表示あり・固定・単位行列です。位置はIPT生成前のsolidに反映してください。
STEPから生成したIPTには世界座標の形状が入っているため、IAMで再度変換しません。
同じIPTを追加すると、参照定義を共有した配置が同じ位置に増えます。
配置名は大文字小文字を区別せず一意である必要があります。省略時は部品表示名と部品ごとの
配置番号を使います。同じ表示名の異なる部品には `name` を明示してください。

`add_part` はIPTのbyte snapshotを保持します。その後元ファイルを変更・削除しても、
出力には保持した内容を使います。`filename="rail.ipt"` で同梱ファイル名を指定できます。
Windowsで同名になる異なるファイルや、同じ文書IDなのに内容が異なるIPTは拒否します。

`save_bundle` はIAM・内容を変更しないIPT・`bundle.json` を新規フォルダーに出力します。
全体の構築後に排他的なdirectory renameで公開します。親フォルダーは事前に必要です。
既存のファイル・フォルダー・symlinkは上書きしません。移動時はフォルダー全体をまとめてください。

reportにはファイル名・サイズ・SHA-256・identity・配置を記録します。
`native_validation="not_run"` は今回の個別出力の状態です。生成処理はInventorを起動しません。
繰り返し保存するとアセンブリのidentityは毎回新規になります。

## CLI

```sh
python -m inventor_kit write-assembly frame_bundle parts/rail.ipt parts/plate.ipt \
  --name Frame --part-number ASM-001 --filename frame.iam --timeout 120
```

新規出力先の後に、配置順のIPTを渡します。同じpathを繰り返すと再配置になります。
JSONを標準出力へ返し、成功時は終了code 0です。独立workerの正常終了を確認してから出力します。
timeout・非対応入力・worker異常時には出力先を作成しません。

32部材以下のSTEP変換結果からIAMを作る例:

```python
from pathlib import Path
import inventor_kit as ik

ik.write_step_parts("frame.step", "frame_parts")
assembly = ik.new_assembly(name="Frame")
for index, path in enumerate(sorted(Path("frame_parts").glob("*.ipt")), 1):
    assembly.add_part(path, name=f"Part:{index}")
assembly.save_bundle("frame_bundle", filename="frame.iam")
```

## Rustと制限

`inventor-core` の `iam-writer` featureを有効にし、
`PartDefinition::from_authored_ipt`、`AssemblyDocument::add_part`、`encode` を使います。
Rust APIはメモリー内で動作するため、IAMと同じフォルダーへ入力と同一のIPTを置く責任は呼出側にあります。
`prepare()` でidentityと配置を取得し、`PreparedAssembly::encode()` で検査・生成できます。
従来の `from_ipt` は計画用identity検査として残りますが、IAM生成にはより厳しい
`from_authored_ipt` で作成した定義が必要です。

| 項目 | 対応範囲 |
|---|---|
| Profile | `inventor-iam-rse31-fixed-parts-v1`、mm、RSe31、schema15参照 |
| IPT | writerの初期保存profile、1件64 MiB以下 |
| Bundle | 32配置以下、PythonではIAM・reportを含み256 MiB以下 |
| 配置 | 平坦・固定・表示あり・単位行列 |
| 参照 | 同じフォルダーのIPT名、同一名の定義を共有 |
| 対象外 | 拘束・joint・入れ子IAM・任意変換・表示切替・native再保存IPT入力・既存文書の更新 |
| 実機受入 | 公開writer生成4条件＋対照1条件、35フェーズ、build `311270010` |

実機では1部品、2定義3配置、同一部品32配置、別の32部材アセンブリを検査しました。
各条件でopen/save/reopen、フォルダー移動後の生成IAMと保存IAMの解決、再保存・再読込が通り、
保存文書のidentity・参照・形状トポロジー・boundsを照合しました。32部材は初期表示・fit・
移動後再読込の画面も確認済みです。Inventor保存後は参照が絶対パスになることがありますが、
元のフォルダーを移動してから参照解決を検査しています。他のInventor buildは未検証です。

`bundle.json` の `document.native_qualified` は限定profileの検証状態です。
新規出力ごとの `native_validation="not_run"` は維持します。Linuxでwheel・sdistの検査を実施し、
この変更のリモート全OS CIは未実施です。

実装はtyped fieldと生成規則から構築します。原本CAD・元文書identity・取得script・oracle・
調査報告は公開source/binaryに含めず、公開テストは合成部品を生成します。
有効状態と制限は `ik.capabilities()["assembly_writer"]` で確認できます。
[ライセンス](license.ja.md)は既存機能と同じです。UUID v5実装の依存noticeも同梱します。
