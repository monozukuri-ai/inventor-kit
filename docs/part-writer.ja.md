# Inventor部品を書き出す

[English](part-writer.md) | 日本語

実験段階のwriterは、Inventor・Inventor API・既存IPTテンプレートを使わず、単独の `.ipt` を生成します。
1部品につき1 solidを受け取り、正確な形状を非パラメトリックな基本フィーチャとして保存します。
表示用メッシュも別に保存します。既存ファイルやディレクトリは上書きしません。

このcheckoutには `cq-acis>=0.3.12,<0.4` が必要です。未公開の変更を試す場合は、
[配布ワークフロー](https://github.com/monozukuri-ai/inventor-kit/actions/workflows/release.yml)
の候補wheelをインストールしてください。パッケージ版数は[Cargo.toml](../Cargo.toml)で管理します。


## 出力するInventorの版

既定の出力は2027です。2026向けには `new_part(..., target_year=2026)`、
`write_step_parts(..., target_year=2026)`、`new_assembly(..., target_year=2026)` を使用します。
CLIの `write-step` と `write-assembly` は `--target-year 2026` に対応します。
IAMと参照するIPTは同じ出力版にそろえてください。混在する入力は生成前に拒否します。
2027ファイルを読み込んで2026へ変換する機能ではありません。

2026はRSeコンテナーschema 31、segment major 30、ASM 23100です。
統合後の公開API/CLIで生成した32部材と1階層のIAMは、2026-10-16に
Inventor 2026.4（build `304435000`）で保存・再読込、フォルダー移動後の参照解決・再保存・再読込に合格しました。
形状SABは追加加工にも合格した先行生成版と完全一致していますが、統合版の単品追加加工は再実施していません。
これは限定profileの検証結果であり、個々の出力を実機検査するものではありません。
各出力の `native_validation` は常に `not_run` とします。2027の既存profileは維持しています。

## CadQueryから部品を作る

```python
import cadquery as cq
import inventor_kit as ik

solid = cq.Workplane("XY").circle(10).extrude(20).val()
part = ik.new_part(name="円筒部品", title="検証部品", part_number="CYL-001")
part.add_body(solid, name="本体", feature_name="基本形状")
report = part.saveas("円筒部品.ipt")
print(report.path, report.report_path, report.native_validation)
```

`new_part` は `name`、`title`、`part_number`、`units="mm"`、任意の `target_profile` と `target_year` を受け取ります。両方を指定する場合は対応する版をそろえてください。
`add_body` の入力はmm単位の有効なCadQuery `Solid` 1個です。
名前・Title・Part NumberはUnicodeに対応し、NUL・不正なサロゲートを含まない255 UTF-16単位以内です。
文書・body・feature名は空にできません。Inventorに表示される文書名はファイル名に従います。

`saveas` はIPTと `<path>.json` を作成し、hash・属性・境界・トポロジー・検証範囲を記録します。
両方のpathが未使用で、親ディレクトリが存在する必要があります。生成後に2ファイルを配置するため、
プロセス中断時には片方だけ残る場合があります。不正な入力は `PartWriteError` で拒否します。

## STEPの各部材を個別IPTに保存する

```python
import inventor_kit as ik

result = ik.write_step_parts("assembly.step", "output_parts")
print(result.part_count, result.report_path)  # output_parts/batch.json
```

STEPはmm単位を明示している必要があります。各solidの配置につき単独IPTを生成し、元のworld配置を1回だけ適用します。
同じ定義の繰返し配置も個別ファイルとして残します。XDE製品名をbody名・Title・Part Numberに設定します。
名前はOCCTが読み取った値で、STEP文字列の前後の空白が正規化される場合があります。
ファイル名は連番とWindows対応の名前を組み合わせ、元の名前・変換行列・対応表を `batch.json` に記録します。

出力先は未作成のディレクトリを指定し、その親は存在している必要があります。
全件を一時領域で生成し、トポロジー・体積・面積・境界・形状差を検査してから出力先を確定します。
既存の空ディレクトリ・ファイル・symlinkは保持します。
失敗時の `StepPartWriteError.report` には工程と、生成中なら対象部材を記録します。
非対応形状、外部文書、不正な配置、入力の途中変更、上限超過では一式を出力先に配置しません。

CLIでも同じ変換を実行できます。

```sh
python -m inventor_kit write-step assembly.step output_parts --timeout 600
```

結果をJSONで表示し、成功時は終了コード0、失敗時は1を返します。timeoutは形状処理を含む変換全体に適用します。
workerが正常終了してから出力ディレクトリを確定します。成功時は `batch.json` と各IPTのレポートを保存し、
失敗時は標準出力のJSONにエラーを記録します。APIを使う場合、実行時間・メモリ量の制限が必要なら別プロセスで監視してください。

## 対応範囲と制限

| 項目 | 対応範囲 |
| --- | --- |
| 形状 | 閉じた有効な単一solid。平面・円筒面、直線・円・楕円の辺 |
| 単位 | 明示的なmm |
| 文書 | 非パラメトリックな基本フィーチャ1個、新しい独立した文書ID |
| 属性 | 文書ラベル、body・feature名、Title、Part Number |
| STEP一括変換 | 名前、入れ子の剛体配置、同一定義の繰返し配置 |
| 対象外 | IDW出力（IAMは別のアセンブリwriterを使用）、複数body IPT、元のフィーチャ履歴の復元、外観の移植、一般NURBS面 |

`StepPartLimits` の既定値は入力32 MiB、STEP entity 250,000、部材256、階層node 4,096、深さ32、
面・辺・頂点の合計32,768、出力256 MiBです。各部材は面と辺の合計512件までです。
表示配列とメッシュ処理の総量にも別の上限があります。非対応入力は拒否し、メッシュ置換や部材の黙示的省略は行いません。

穴付き部品の一部の回転配置など、入力自体が有効な解析形状でも、OCCTが有効な形状差を計算できず
往復検査に失敗する場合があります。その一式は拒否し、体積・境界が一致するだけでは成功としません。

profileは `inventor-ipt-rse31-metric-analytic-base-feature-v21` です。
Inventor 2027.1 build 311270010で、解析形状11部品と別の32部材アセンブリから生成した個別IPTを検証しました。
通常表示・更新・保存再読込に加え、解析形状11件とアセンブリの代表4部材への追加加工を確認しています。
保存形状・属性・配置・InventorからのSTEP再出力も独立に比較しました。
これは限定した開発用形状集合の検証であり、すべての入力・配布artifact・Inventor版の受入を意味しません。

各新規出力の `native_validation` は `not_run` です。保存処理からInventorは起動しません。
`ik.capabilities()["part_writer"]` で対応範囲と実行extensionのwriter有効状態を確認できます。
配布検査は合成入力を使用し、Inventor実機での受入とは分けています。

通常のPythonビルドではRustの `analytic-writer` featureを有効にします。
Rustから読取り機能だけを使う場合はdefault featuresを無効にできます。
一括出力の排他的な確定にはLinuxの `renameat2`、macOSの `renamex_np`、Windowsのdirectory renameを使います。
filesystemが対応しない操作はエラーになります。
