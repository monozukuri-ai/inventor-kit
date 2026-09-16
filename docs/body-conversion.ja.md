# body選択と変換レポート

[English](body-conversion.md) | 日本語

v0.4.0のbody APIは、IPTに保存された各bodyをcq-acisで個別に変換し、未対応bodyと出典も保持します。
既存の `doc.to_cadquery()` は従来どおり部品全体の変換です。現在のModel Stateは未検証です。

```python
import inventor_kit as ik

doc = ik.read_file("part.ipt")
result = doc.convert_bodies()
for body in result.bodies:
    print(body.id, body.body_index, body.status, body.diagnostics)
# この一覧からIDを選びます。主bodyを自動選択することはありません。
# result.export_step("selected.step", body_ids=[chosen_id], allow_partial=True)
```

`BodyConversion` は `id`、`body_index`、`source`、`status`、`diagnostics`、`metrics`、`shape` を持ちます。
shapeと形状量があるのは `converted_solid` だけです。その他は `open_shell`、`unsupported`、`failed` です。
shellを生成できないsewing失敗は `unsupported` とし、有効なsheet bodyとは判定しません。
開いた面や個別面の集合をソリッドとして出力しません。

IDは入力SHA-256、保存候補、元body indexに結び付きます。同じバイト列のコピーでは同じIDになります。
別入力・別候補のID、重複ID、明示した空選択は拒否します。
SourceSpanの位置は共通ACISモデルと同じく、展開後Bストリーム内の位置です。

`result.report(body_ids=...)` はOCCTオブジェクトを含まないJSON化可能な結果を返します。
[v1スキーマ](../schemas/conversion-report-v1.schema.json)は `AssemblyConversion.report()` と
[CLI](cli.ja.md)の結果も扱います。診断codeはcq-acisの値を維持し、取得できる出典・entity・subtypeを付けます。
不明な詳細はnullです。予期しないnative例外は呼出し元へ伝播します。

| フィールド | 意味 |
| --- | --- |
| `geometry_complete` | 一覧の全bodyが有効ソリッドに変換できた |
| `selection_complete` | 要求した全bodyが有効ソリッドに変換できた |
| `current_state_verified` | この版では常にfalse |
| `complete` | 常にfalse。保存形状だけでは現在状態を証明できない |
| `omissions` | 選択しなかったbodyと変換できないbody、その診断 |

`to_cadquery(body_ids=..., allow_partial=True)` と
`export_step(path, body_ids=..., allow_partial=True)` は明示した選択を使用します。
`body_ids` を省略すると全bodyを要求します。除外bodyがある場合は `allow_partial=True` が必要です。
有効ソリッドを一つも含まない選択は常に `BodyConversionError` で拒否し、例外の `result` に全結果を残します。
レポートとbody一覧の確認だけなら部分出力の許可は不要です。

STEP出力は新しい `.step`/`.stp` と `.json` 付属ファイルを作成します。
XDEで再読込し、名前、bodyの対応、面・ソリッド数、体積、面積、境界箱を確認します。
付属JSONには入力hash、候補、body ID、除外一覧を残します。
既存ファイルは拒否し、往復検査が失敗した場合は両方とも出力しません。

FTC06（2021/2024）ではbody 1が146面の有効ソリッドですが、補助平面bodyが二つあるため全体変換は停止します。
明示選択でbody 1を取り出し、残る二つを記録できます。これは部品全体の変換成功数には加えず、
Inventorで現在表示されるbodyの推定にも使いません。

```sh
python -m inventor_kit part.ipt --list-bodies
python -m inventor_kit part.ipt --body-id BODY_ID --allow-partial --step selected.step
python -m inventor_kit.viewer part.ipt --body-id BODY_ID --allow-partial
```

viewerは未対応・未選択bodyもツリーに残します。部分表示には `--allow-partial` が必要で、画面にも部分表示と示します。
