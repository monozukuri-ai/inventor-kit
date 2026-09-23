# IDW読み取り・保存図面の表示

[English](drawing.md) | 日本語

対応するIDWは、通常のViewer起動で開けます。
Inventorや参照モデルの再投影を使わず、保存された2D要素を読み取ります。
**実験的な部分対応**で、schema31 / Meta8のmajor31（zstd）とmajor23（zlib）が対象です。
図面全体の正確性、物理単位、シート所属の完全性、現在状態は未検証です。
IDW内の保存表示を使用し、関連するIPT/IAMの読み込みやビューの再生成は行いません。

```python
from inventor_kit import read_drawing_file

drawing = read_drawing_file("drawing.idw")
print(drawing.status, drawing.units, drawing.diagnostics)
for sheet in drawing.sheets:
    print(sheet.id, sheet.index, sheet.name, sheet.size_in_source_units)
    print(sheet.status, len(sheet.items), sheet.omissions)
    for item in sheet.items:
        print(item.id, item.geometry["kind"], item.source)
```

`read_drawing(bytes, source_id=..., limits=..., drawing_limits=...)` はバイト列を、
`read_drawing_file(path, limits=..., drawing_limits=...)` はサイズ上限付きでファイルを読み取ります。
戻り値の`DrawingDocument`、シート、要素、画像情報、geometry/style内の入れ子も変更不可です。
図形種別は`polyline`・`curve`・`text`・`image`。埋め込みPNG/JPEGは原本バイト列を保持します。
対応するmajor23のスプラインは、保存された次数・節点・制御点・重み・区間から
各非空節点区間を16分割して`polyline`として表示します。これは近似表示で、幾何誤差の保証ではありません。
楕円弧は`curve`の中心・2軸・角度区間として保持します。
対応するmajor31の白黒ビュー画像とmajor23のカラーRGBAビュー画像は保存画素からPNGへ変換し、そのSHA-256と元レコードの出典を保持します。
出典のバイト範囲と省略理由も取得できます。`drawing.sheet(id)`でシートを選択します。
同名シートを許容するため名前は識別キーにしません。IDは入力SHA-256と保存record・配置の識別情報を含み、異なる入力のIDを拒否します。
ファイルの移動では変わりませんが、
Inventorでの再保存をまたぐ安定性は保証しません。`api_version=1`、対応状態は実験段階です。

シートは保存された文書内の順序で並び、一意に所属を確認できる表示要素を含みます。
表示内容が欠落している、または所属が曖昧なシートは`unavailable`となり、診断に理由を残します。

`size_in_source_units`は原値の幅・高さです。`units=source_units_unverified`、
`length_unit=None`、`millimeters_per_unit=None`を維持しています。
[Autodesk APIの内部長さ単位はcm](https://help.autodesk.com/cloudhelp/2021/ENU/Inventor-API/files/UOM_Overview.htm)ですが、
それだけでは今回の保存形式の単位を証明できません。cm→mmの自動変換や寸法測定は提供しません。
表示できても`qualified=False`、`complete=False`、`current_state=unverified`です。

```sh
python -m inventor_kit.viewer drawing.idw
```

通常のローカルViewerで、シート選択、パン、拡大、全体表示、文字・曲線の表示切替、文字検索、
要素の出典確認ができます。IDW表示にはPythonの`viewer` extraは不要で、ブラウザーの3D描画器も読み込みません。
文書情報と保存プレビューだけを確認する場合は `--metadata-only` を指定します。
従来の `--experimental-drawing` は互換性のため受理しますが、指定は不要です。
`--allow-partial` を付けてもIDWの表示や単位の検証状態は変わりません。
原座標の保存内容を表示し、画面には部分表示・単位と現在状態が未検証であることを表示します。
同名シートには一覧で順番を添え、最初に表示可能なシートを開きます。
CLIの依存判定も含め、識別はCFB root CLSIDによります。
文書種別と両立しないオプションはworkerの診断として表示します。
IPT/IAM選択オプションとの併用はできません。既存のプロセスタイムアウト、シーンJSON 16 MiB上限、
画像のバイト・画素数上限とViewerバッファ上限を適用します。workerが正常終了した後に図面と画像を公開します。
選択したシートのJSONは32 MiBまで、図面のシート・画像リソースの合計は128 MiBまでです。

未対応majorは属性・保存サムネイル・診断を残します。CFB破損、非IDW入力、不正な上限値は`ValueError`になります。
各native処理段階が独立して`Limits`を適用します。プロセス全体のメモリ量や実行時間の保証ではありません。

`DrawingLimits`で図面専用のnative処理上限を指定できます。既定値が最大値で、
0以上の整数へ引き下げられます。boolは受理しません。

```python
from inventor_kit import DrawingLimits, read_drawing_file

drawing = read_drawing_file("drawing.idw", drawing_limits=DrawingLimits(
    max_sheets=32, max_views=128, max_display_items=20_000, max_output_bytes=16 * 1024 * 1024))
```

| フィールド | 既定値・最大値 | 集計範囲 |
| --- | ---: | --- |
| `max_sheets` | 256 | 保存シート参照数・候補表示空間数 |
| `max_views` | 4,096 | 図面全体で到達・解読できた保存ビュー配置 |
| `max_display_items` | 100,000 | 図面全体の配置展開後の要素数 |
| `max_polyline_points` | 1,000,000 | 図面全体の配置展開後の折れ線点数 |
| `max_text_bytes` | 16 MiB | 展開後のUTF-8文字列・ビュー名・複製するfont名 |
| `max_reference_visits` | 3,000,000 | シート所属・表示展開・画像取得の各native段階で、図面全体の参照・複製作業量 |
| `max_nesting_depth` | 128 | 表示・祖先参照の探索深さ |
| `max_image_bytes` | 16 MiB | 配信する画像の合計バイト数。ビュー画像は生成PNGを集計 |
| `max_image_pixels` | 16,777,216 | 埋め込み画像の合計画素数 |
| `max_output_bytes` | 96 MiB | metadata・画像データを含むnative図面JSONの出力バイト数 |

要素・点・文字は複製前に予算を消費し、同じ定義の複数配置も集計します。
展開予算を超えると表示を破棄し、metadataと診断を残します。画像上限は表示できない画像として記録します。
JSON出力はバッファへ追記する前に検査し、超過時は`ValueError`になります。
containerと型付きフィールドの解析には、それぞれ`Limits.max_records`を上限として使います。
ビュー画像の元画素はストリーム展開量と`max_image_pixels`で制限し、同一ストリームは1回だけ展開します。未知のビュー形式の数、RSS、実行時間の保証ではありません。
Viewerは既定値を使い、Pythonの`Options`では`drawing_limits`を指定できます。

## 表示する内容と制限

直線・ポリライン・円・円弧・文字・画像の保存要素を表示します。
寸法、引出線、図枠、表題欄、単純な部品表は、対応する保存要素で表現されている場合に表示できます。
寸法や表の完全な意味情報を取得するAPIではありません。

対応する白黒・カラーRGBAビュー画像では、基本図・投影図・尺度・回転・隠線・切り抜きを保存解像度で表示します。
拡大してもベクトル輪郭の詳細は復元できません。未対応の画素配置・半透明形式は表示できない画像として扱い、
未解釈の表示属性がある場合は該当部分を省略して診断に理由を残します。

対応するArial/Tahoma文字はブラウザーのフォント寸法で高さを調整し、空白・個別の基線と
対応する太字・斜体を表示へ反映します。日本語にはローカルのNoto Sans CJK JP・Yu Gothic・Meiryoをsans-serifより先に代替指定します。
未知の文字配置や高さ補正に対応しないブラウザーでは、未検証の代替表示であることを示します。
文字幅・字形はインストール済みフォントに依存し、完全な一致は保証しません。
観測済みのAIGDT `n`はUnicode `⌀`に近似表示し、原文とフォント名をAPIに保持します。
直径記号の代替表示にはローカルの記号用フォントを指定します。他の専用記号は対応していません。

一般的なクリッピング・重なり順・文字整列・寸法や注記の網羅性は未検証です。
円・楕円弧は固定分割の折れ線からSVGの楕円弧へ変更し、アフィン変換や解読済みの破線長を反映します。
投影後にほぼ直線となる楕円は端点と座標の極値を結ぶ線分で表示し、SVG・付属JSONへ記録します。
この処理の解析的な偏差上限は2e-9原単位で、数値の文字列化・ブラウザー描画の誤差は含みません。
スプラインは前述の近似表示です。未知の線種maskは未解釈のままで、native線種全般への対応を意味しません。
major24/26/28/29はこの描画プロファイルの対象外です。
major23では保存カラー画像の上に対応する輪郭線・注記を重ねます。
古い保存リビジョンへの参照は同じオブジェクトであることを確認して結び付けますが、過去状態の再現は保証しません。
未対応のスケッチ状態や別セグメントの表示子要素、欠落画像は省略理由として残ります。

## 表示要求とシート配信

`drawing.render_sheet(sheet_id=sheet.id, allow_partial=False)` は、mm単位の表示データを要求する入口です。
無効なシートID・他の入力のIDは、コード`drawing.invalid_sheet_id`の`DrawingDisplayError`になります。
単位・配置が未確定の場合は、`allow_partial=True` でも `DrawingDisplayError` を返します。
例外の `sheet_id`、`diagnostics`、`omissions` から理由を確認できます。
現行major23/31は物理単位が未検証なので、この入口での表示はまだ受理されません。
`sheet.items` と通常のViewerは、物理単位へ変換せず、対応する保存内容を原座標で表示します。
`content_coverage=unknown`、`snapshot_kind=saved`、`reference_freshness=unverified` を保持します。

Viewerは選択したシートを必要に応じて読み込み、切替前の古い応答は表示しません。
シートと画像のリソースを検査してから配信します。
シートJSONは個別32 MiB、シートJSONと画像の合計は最大128 MiB（Viewerのbuffer上限で縮小可能）です。
metadataの16 MiB上限は別に維持します。これらはRSSや実行時間の保証ではありません。

### 保存ビューの読み取り

`sheet.views` は、変更不可の `DrawingView` を返します。入力に結び付いたID、
名前、キャッシュ境界、配置行列、画像参照、対応する要素ID、出典を取得できます。
Viewerのビュー一覧から、対応する保存要素を選択・強調表示できます。
配置行列はキャッシュの配置を表し、画像に焼き込まれた回転角は表しません。
`rotation`・`parent_view_id`・`view_type` は未解読の場合 `None` です。
キャッシュがない場合は `image_reference` と `cache_bounds` が `None` になります。
境界と行列は原座標を保持し、切り抜き範囲の確定には使いません。
`max_views` は解読できた配置数を図面全体で制限します。未知のビュー形式は省略として残ります。

## レポートとオフラインSVG（開発版）

```python
from inventor_kit import read_drawing_file

drawing = read_drawing_file("drawing.idw")
report = drawing.report()  # JSON化できる一覧、原文、省略理由
sheet = drawing.sheets[0]
detailed = drawing.report(sheet_id=sheet.id, details=True)
svg = drawing.to_svg(sheet_id=sheet.id, allow_partial=True)  # str
saved = drawing.export_svg("sheet.svg", sheet_id=sheet.id, allow_partial=True)
```

`export_svg`はSVGと`.svg.json`を新規作成し、どちらも上書きしません。
付属JSONは[図面レポートv1](../schemas/drawing-report-v1.schema.json)で、入力・出力のハッシュ、
原文とfont情報、保存ビュー参照、出典、省略理由を記録します。画像バイト列はSVGに埋め込み、JSONには含めません。
`to_svg`・`export_svg`は部分出力への明示的な同意が必要です。`max_bytes`は32 MiB以下に制限でき、
複数シートの文書ではID指定が必要です。表示不能なシートや別入力のIDは`DrawingDisplayError`になります。
APIの`report()`は辞書を返し、CLIのJSON化はサイズ上限付きで、複数入力のJSONLにも対応します。

Viewer・Python API・CLIは共通のSVG描画処理を使います。
Viewerの**Save partial SVG**で画像を埋め込んだ選択シートを、**Save report**で出典JSONを保存できます。
拡大、検索、選択、文字・曲線の非表示操作にかかわらず保存シート全体を出力します。
Viewerのサーバーを停止した後もネット接続なしで開けます。
SVGには入力とシートの識別情報を含め、未検証の原座標とピクセルの表示領域を使用します。mmを意味しません。
フォントは埋め込まないため、インストール済みフォントやブラウザーの高さ補正対応によって表示は変わります。
XMLで扱えない文字はSVG内のみ置換し、JSONには原文を保持します。
SVG保存によってmm単位の`render_sheet`が利用可能になるわけではなく、現在状態や注記の意味も未検証です。

## 単位精度と一般線種の検証

[精度測定器](../scripts/measure_drawing_precision.py)は、固定した4単位制御図面について、
保存座標・APIのcm値・SVGの線座標・native PDFの座標を別々に比較します。
縮尺合わせや位置合わせは行わず、0.001 mmの判定と実測誤差を記録します。
既存の400 dpi PDFでは線座標に最大0.028 mm、用紙に最大約0.139 mmの差があり、
精度基準に使えません。保存座標とAPI値の一致は、この4つの保存入力だけの証拠です。
一般IDWの物理単位や印刷精度の認定にはなりません。実行には非配布の取得済み制御図面、
`pdfinfo`、`mutool`が必要です。

```sh
python scripts/measure_drawing_precision.py --input /path/to/units --output precision.json
```

[線種採取スクリプト](../scripts/create_drawing_linetype_controls.ps1)は、標準15線種の
レイヤー継承・個別上書きと、線幅・尺度・線幅連動を分けた38ケースのIDW/API/PDFを新規作成します。
Windows PowerShell 5.1で、Inventorを起動し既存文書をすべて閉じて実行します。
既存のIDWやグローバルスタイルは更新しません。取得後の検査も認定とは別です。
Inventor 2027.1（major31）で38ケースを採取・照合し、実験的decoderに標準15線種の
レイヤー配列と線幅連動を追加しました。レイヤー・bindingの尺度が観測済みの1である場合に限定します。
個別指定の保存配列には既に倍率が含まれるため、二重に拡大しません。
保存IDと[APIのLineTypeEnum](https://help.autodesk.com/cloudhelp/2024/ENU/Inventor-API/files/LineTypeEnum.htm)は
別の数値体系です。名前や列挙値だけからdash配列を割り当てません。

```powershell
.\scripts\create_drawing_linetype_controls.ps1 -OutputDirectory C:\Evidence\linetypes-new
```

```sh
python scripts/validate_drawing_linetype_controls.py --input /path/to/linetypes-new --output acquisition.json
```

出力先は新規のパスを指定します。未取得・getter失敗・保存後変更・重複・入力ハッシュ不一致を拒否します。
採取検査は成功時も`pattern_mapping_qualified=false`を維持します。
固定した実データでdecoderの公称配列とPDF直線モデルを別に検証します。

```sh
python scripts/measure_drawing_linetypes.py --input /path/to/linetypes-new --output linetypes.json
```

native PDFは線の端点に合わせて破線周期・位相を調整します。SVGは公称配列を保持し、
`dash_phase_and_fit_unverified` を報告します。38ケースのPDF直線モデルは0.01 mm基準で一致しましたが、
曲線の位相、major23のレイヤー線種、カスタム`.lin`、未観測の尺度、独立holdoutは未検証です。
スケッチへの既定値の明示設定が実線の上書きを作るため、collectorは継承プロパティに触れません。
観測済みmajor31の既定線幅値はレイヤー幅を継承し、シート全体を表示不能にする問題も修正しました。
