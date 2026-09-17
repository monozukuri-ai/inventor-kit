# 実験的なIDW読み取り・表示

[English](drawing.md) · [構造一覧](drawing-inventory.ja.md)

Inventorや参照モデルの再投影を使わず、保存された2D要素を読み取ります。
**実験的な部分対応**で、現在の実図面検証はSampleBgのmajor31 / schema31 / Meta8 / zstdに限られます。
図面全体の正確性、物理単位、シート所属の完全性、現在の保存状態は未検証です。

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

`read_drawing(bytes, source_id=..., limits=...)` はバイト列を、
`read_drawing_file(path, limits=...)` はサイズ上限付きでファイルを読み取ります。
戻り値の`DrawingDocument`、シート、要素、画像情報、geometry/style内の入れ子も変更不可です。
図形種別は`polyline`・`curve`・`text`・`image`。画像は元のPNG/JPEGバイト列とSHA-256を保持します。
出典のバイト範囲と省略理由も取得できます。`drawing.sheet(id)`でシートを選択します。
同名シートを許容するため名前は識別キーにしません。IDは同一入力内で安定しますが、
Inventorでの再保存をまたぐ安定性は保証しません。`api_version=1`、対応状態は実験段階です。

シート順序は文書内のタグ付き参照一覧に従います。各定義とSM表示空間の逆参照を、
context、segment GUID、完全なobject keyで照合し、一意に結び付く場合だけ表示対象にします。
registryの順序、表示名、サムネイルから所属を推定しません。未接続の表示空間は診断に残して除外し、
曖昧・欠落したシート表示は`unavailable`になります。

`size_in_source_units`は原値の幅・高さです。`units=source_units_unverified`、
`length_unit=None`、`millimeters_per_unit=None`を維持しています。
[Autodesk APIの内部長さ単位はcm](https://help.autodesk.com/cloudhelp/2021/ENU/Inventor-API/files/UOM_Overview.htm)ですが、
それだけでは今回の保存形式の単位を証明できません。cm→mmの自動変換や寸法測定は提供しません。
表示できても`qualified=False`、`complete=False`、`current_state=unverified`です。
構造一覧側の適格化済みシート数を表す`sheet_count`もnullのままです。

```sh
python -m inventor_kit.viewer drawing.idw --experimental-drawing
```

通常のローカルViewerで、シート選択、パン、拡大、全体表示、文字・曲線の表示切替、文字検索、
要素の出典確認ができます。IDW表示にはPythonの`viewer` extraは不要で、ブラウザーの3D描画器も読み込みません。
フラグなしでは文書情報・保存プレビューと有効化方法を表示します。識別は拡張子ではなくCFB root CLSIDによります。
IPT/IAM選択オプションとの併用はできません。既存のプロセスタイムアウト、シーンJSON 16 MiB上限、
画像のバイト・画素数上限とViewerバッファ上限を適用します。workerが正常終了した後に図面と画像を公開します。

未対応majorは属性・保存サムネイル・診断を残します。CFB破損、非IDW入力、不正な上限値は`ValueError`になります。
各native処理段階が独立して`Limits`を適用します。プロセス全体のメモリ量や実行時間の保証ではありません。

ローカル検証はSampleBg、参照破損・曖昧性、合成データでの複数シート順序・同名、未対応profile、
上限、worker異常終了、ブラウザー操作を対象とします。合成テストは実際の複数シートの正確性を証明しません。
追加取得した公開IDWは別majorのため、別のmajor31図面・制御差分・Windows/Autodesk比較は未完です。
major23/24/26/28/29はこの描画profileでは受理しません。
クリッピング、重なり順、文字寸法・整列、一部のスタイル、寸法・注記の網羅性は未確定です。
Viewerの曲線分割と代替フォント・色は表示用の近似で、APIには元の曲線パラメーターを保持します。
