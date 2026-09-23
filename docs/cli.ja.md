# CLIとバッチ変換

[English](cli.md) | 日本語

従来の単一入力の検査コマンドも利用できます。形状変換・変換レポートはv0.4.0以降、IDWレポート・SVG保存はv0.5.0以降で利用できます。

```sh
python -m inventor_kit part.ipt
python -m inventor_kit drawing.idw --metadata-only
python -m inventor_kit part.ipt --list-candidates
python -m inventor_kit part.ipt --list-bodies
python -m inventor_kit part.ipt --convert --report part-report.json
python -m inventor_kit part.ipt --step part.step
python -m inventor_kit assembly.iam --step assembly.step --allow-unverified-state
python -m inventor_kit a.ipt b.ipt --output-dir converted --jsonl --report results.jsonl
```

`--step` と `--output-dir` は変換を行います。文書種別はファイル内容から判定します。
IAMは既存resolverを使い、部品の検索先は `--search-root` を繰り返して指定できます。
保存配置には `--allow-unverified-state`、欠落を含むIPT/IAMの出力には `--allow-partial` を指定します。
候補IDとbody IDは一つのIPT入力に限定します。複数bodyは `--body-id` を繰り返して選びます。
[body選択](body-conversion.ja.md)も参照してください。

`--list-bodies` は個別変換を試み、ファイルへ出力せずに全bodyを報告します。
`--metadata-only` と `--list-candidates` はPythonの形状ライブラリをimportしません。
新しい変換・レポート指定がなければ、単一入力は従来のsummary形式です。
検査・形状変換の`--report`、`--jsonl`、変換モードは [変換レポートv1](../schemas/conversion-report-v1.schema.json)を使用します。
複数入力でJSONL指定がなければJSON配列を出力します。

| 終了code | 結果 |
| --- | --- |
| 0 | 全ジョブが成功 |
| 2 | 部分形状・部分選択。除外一覧を確認 |
| 3 | 未対応の入力・形状で利用可能な結果がない |
| 1 | 不正な指定・入力、出力拒否、timeout、プロセス異常、出力失敗 |

複数入力ではエラー、未対応、部分結果の順に優先して終了codeを返します。
従来形式の単一入力検査は、未対応profile/stateをsummaryに返す場合も従来どおり終了code 0です。
上表の終了codeを使うには `--report` または `--jsonl` を指定してください。
従来の単一部品 `--step` も、新しいレポート・選択指定がなければ標準出力はsummary形式を維持します。
出力計画自体が不正な場合を除き、他の入力の処理も続けます。
部分結果のレポート確認には `--allow-partial` は不要ですが、部分STEP出力には必要です。
形状の成功と現在状態は区別し、現在状態は未検証のままです。
CLIレポートにはinspection/conversion/exportの処理段階も付きます。

各入力を別プロセスで実行し、native変換を含めて既定120秒で打ち切ります。
変更する場合は `--timeout SECONDS` を指定します。native出力はJSONの標準出力へ混入しません。
異常終了・timeoutは分類した結果にし、後続入力を処理します。
STEPは一時領域で生成し、子プロセスの正常終了後に出力先へ配置します。OSによるメモリ上限ではありません。

STEPとレポートは上書きしません。バッチのSTEP名は入力の拡張子を除いた名前から作るため、
同名や大文字小文字だけ異なる名前は処理前に拒否します。その場合は出力先を分けてください。
途中の入力が失敗しても、先に成功した出力は残ります。各STEPには出典・除外情報の検証済みJSONが付きます。

## IDWのレポートとSVG保存

```sh
python -m inventor_kit drawing.idw --list-sheets
python -m inventor_kit drawing.idw --drawing-report --report drawing.json
python -m inventor_kit a.idw b.idw --drawing-report --jsonl --report drawings.jsonl
python -m inventor_kit drawing.idw --drawing-report --drawing-details --sheet-id '一覧のID'
python -m inventor_kit drawing.idw --svg sheet.svg --sheet-id '一覧のID' --allow-partial
```

[図面レポートv1](../schemas/drawing-report-v1.schema.json)を使用し、Pythonの形状ライブラリはimportしません。
`--list-sheets`は簡潔な一覧、`--drawing-report`は保存ビュー、原文、style、出典、画像情報、省略理由を返します。
`--drawing-details`を追加すると全要素のgeometryも含みます。`--sheet-id`には一覧にある入力固有のIDを指定します。
同名シートがあるため名前では選択しません。シート指定・SVG保存は単一入力、図面レポートは複数入力のJSONLにも対応します。
シートが一つだけの図面は、SVG保存時のIDを省略できます。

`--svg`は`sheet.svg`と`sheet.svg.json`を生成します。`--allow-partial`が必要で、既存ファイルは上書きしません。
子プロセスの正常終了後に出力先へ配置します。画像を埋め込み、生成時のブラウザー、Node.js、フォントのダウンロードは不要です。
付属JSONには入力・SVGのSHA-256、選択シート、省略理由、文字原文を記録します。
SVGは32 MiBまでです。workerごとのJSON受け渡し上限は詳細レポートが96 MiB、その他は16 MiBです。
最終出力のインデントや複数入力の集約でサイズは増えるため、大量のgeometryには`--jsonl`を推奨します。
原座標と単位未検証の状態を保持し、実寸印刷や寸法測定の精度は保証しません。

現在の対応IDWは部分対応のため、レポート取得・SVG保存が成功しても**終了code 2**です。
未対応profileは3、無効なID、部分出力の拒否、timeout、出力失敗は1です。
レポートだけなら`--allow-partial`は不要です。形状変換、metadata-only、候補・body・IAM選択指定とは併用できません。
