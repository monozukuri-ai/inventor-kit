# 図面コーパスと比較値

[English](drawing-validation.md)

IDWの読み取り・表示に向けた検証基盤です。図面decoderやviewerの追加ではありません。
Autodeskからの実比較値と、一変更ずつ制御した差分IDWは**未取得**です。
Windows用の収集処理は、COM APIを模した合成オブジェクトでのみテストしています。

## コーパス

`fixtures/drawing-manifest.json` で5図面を選択します。
既存のMetaReader由来2件は `fixtures/manifest.json` の行を参照し、hash・family・splitを重複管理しません。
新規データは出典リポジトリのcommit、元パス、byte size、SHA-256、利用条件を固定しています。

| 図面 | split / family | 取得した資料 |
| --- | --- | --- |
| `_Fishing Rod Assembly.idw`、`SampleBg.idw` | regression / `metareader-samplebg` | 既存IDW |
| `Template_IACS.idw` | regression / `iacs-template` | テンプレートとMITライセンス |
| `RIM.idw` | holdout / `forge-rim` | 部品図候補、参照部品、MITライセンス |
| `VISE ASSEMBLY.idw` | holdout / `lfrum-vise` | 組立図候補、参照組立・部品、PDF、GPL-3.0ライセンス |

部品図・組立図の分類は出典の説明とファイル構成によります。
APIによる分類、単純図面としての適格性、参照ファイルの完全性は未確認です。
バイスのPDFは同名・同一commitですが、保存済みIDWとの対応は**未確認**で、正解データとして扱いません。
新規ファイルはGit対象外の `fixtures/public/drawings/` に取得します。
CAD・PDF・ライセンスの実体は配布物に含めず、上流のソースコードも取り込みません。
各プロジェクトのライセンス原文も固定リビジョンから一緒に取得します。

holdoutは参照ファイルとPDFを含む系列全体を予約します。
decoderや許容差の調整に使った場合は系列全体をregressionに移し、独立したholdoutを補充してください。
3つのmanifestを横断し、同一family/hashのsplit混在、重複パス、大文字小文字の別名、危険な相対パスを検出します。

```bash
python scripts/fetch_public_samples.py
python scripts/fetch_assembly_samples.py
python scripts/fetch_drawing_samples.py
python scripts/check_corpus.py
python scripts/validate_drawing_oracle.py --inventory
```

`check_corpus.py` はライセンス・参照ファイルを含む計66アセットを検査します。IDWが66件あるという意味ではありません。
`--inventory` は入力整合性と比較値取得状況を分けて報告します。
初期状態は図面5件・実比較値0件・取得状態 `incomplete` です。
このオプションの終了コードは入力整合性だけを表し、オプションなしでは比較値不足を終了コード1とします。
CIはfixture取得後にオフラインのテストを実行し、Autodeskの起動や実比較値取得の合格を前提にしません。

## Windowsでの収集

対応するAutodesk環境と `oracle` extraを導入したcheckoutから、新しい出力先を指定します。

```powershell
python scripts/capture_drawing_oracle.py --provider apprentice --input "fixtures/public/drawings/rim/RIM.idw" --project-root "fixtures/public/drawings/rim" --output "internal/oracles/rim.json"
```

`--provider inventor` は別のInventor COMアプリケーションを起動し、
`OpenWithOptions(DeferUpdates=True, SkipAllUnresolvedFiles=True, OpenVisible=False)` を使います。
Apprenticeではその図面APIを使います。どちらも一時コピーで収集し、Save/Updateを呼びません。
`--project-root` は相対参照を維持して指定ツリーをコピーし、省略時はIDWだけをコピーします。
上限は10,000ファイル・512 MiBで、シンボリックリンクを拒否します。対象プロジェクトだけを指定してください。
終了後に原本とコピーのhashを照合します。コピーの外に解決された参照のhashは収集しません。
Autodesk自身の参照解決、参照の完全性、起動時更新は別途確認が必要です。

JSONには `scope=opened_document`、照会前後のDirty/RequiresUpdate/DeferUpdates、
`automatic_update=unknown`、provider/build、collector hash、入力一覧、照会エラーを保存します。
保存しなかったことは、移行やメモリ上の更新が起きなかった証拠にはなりません。
`automatic_update=not_occurred` と認定する前に独立した実行記録を残してください。
更新後の表示を、更新前IDWの比較値として合格させることはできません。既存出力は上書きしません。

## 契約とオフライン検証

`schemas/drawing-oracle-v1.schema.json` を独立して追加し、既存3D oracleの契約を維持します。
集合には観測した件数、1始まりの順序、要素ごとの結果を記録します。
失敗した照会は理由を持ち、`value` を持ちません。
`Nothing` は取得不能、Count=0は取得できた空集合です。途中失敗も保持します。
JSONの重複キー、非有限数、件数・順序・source hash・artifact hashの不一致を拒否します。
sourceを指定すると、記録した相対パスからprojectの基準ディレクトリを復元し、参照を含む全コピー対象のhashも照合します。

収集対象は次の範囲です。

- シートの順序・名前・寸法・縦横・サイズ・状態、ビューの配置・尺度・回転。
- 紙面上の直線・円・円弧、可視性・隠線・レイヤー。未対応の幾何型は状態付きで残します。
- sheet/view sketchのローカル座標と、基底点 `(0,0)`、`(1,0)`、`(0,1)` の紙面上の写像。
  写像の解釈は実機検証が必要です。点など未対応のsketch要素も取得不能として残します。
- 一般注記・sketch文字の原文、書式、配置、回転、寸法、フォント。
  寸法の表示文字と位置、表題欄・枠の評価済み文字、部品表セル。

長さはdatabase cm、角度はradのまま保存します。
引出線注記、記号、ハッチ、画像、枠の幾何、表の配置、PDF/PNG出力は今回の収集範囲外です。
Apprenticeで使えないAPIは `unavailable/failed` として残します。
JSONは32 MiB、集合要素の照会は合計100,000件を上限にします。

```bash
python scripts/validate_drawing_oracle.py --capture internal/oracles/rim.json --source fixtures/public/drawings/rim/RIM.idw
# 合成データ、保存状態不明、値・画像不足では失敗する取得ゲート
python scripts/validate_drawing_oracle.py --capture internal/oracles/rim.json --source fixtures/public/drawings/rim/RIM.idw --require-ready
# 各比較値を <IDWのsha256>.json として配置する。未取得では失敗する
python scripts/validate_drawing_oracle.py --captures internal/oracles
```

通常の単独JSON検査が保証するのは契約と入力一致です。providerの真正性や図面表示の正確性ではありません。
`--require-ready` はさらに、実provider、未更新の保存状態の証拠、照会・参照の取得完了、
各シートに対応する同一セッション画像を要求します。この厳しい取得ゲートはdecoderや画像差分検査とは別です。

`visual_artifacts` にはPDF/PNGの相対パス、bytes/hash、source hash、sheet番号、出典、対応状態、DPI、フォント条件を記録できます。
実ファイルはJSONの親ディレクトリから照合し、`--artifact-root` で基準を変更できます。形式の先頭識別子も検査します。
収集スクリプト自体は画像を出力しません。Windows側での独立出力、同一セッションの対応証拠、
ラスタライズDPI、フォント条件の記録が必要です。公開バイスPDFや合成画像を `same_session` に変更してはいけません。

`tests/data/drawing-oracle-synthetic.json` とCOMを模したテストは、契約と失敗処理の検証専用です。
実比較値の取得ゲートを満たしません。残るWindows作業は[差分図面の作成手順](drawing-specimens.ja.md)を参照してください。

## APIの根拠

[Apprentice図面](https://help.autodesk.com/cloudhelp/2024/ENU/Inventor-API/files/ApprenticeServerDrawingDocument.htm)、
[open options](https://help.autodesk.com/cloudhelp/2024/ENU/Inventor-API/files/Documents_OpenWithOptions.htm)、
[紙面上の曲線幾何](https://help.autodesk.com/cloudhelp/2024/ENU/Inventor-API/files/DrawingCurveSegment_Geometry.htm)、
[sketchから紙面への写像](https://help.autodesk.com/cloudhelp/2024/ENU/Inventor-API/files/DrawingSketch_SketchToSheetSpace.htm)、
[database単位](https://help.autodesk.com/cloudhelp/2021/ENU/Inventor-API/files/UOM_Overview.htm)、
[表題欄の評価済み文字](https://help.autodesk.com/cloudhelp/2024/ENU/Inventor-API/files/TitleBlock_GetResultText.htm)
を参照して収集処理を作成しました。API仕様の確認と、特定のWindows環境での動作確認は区別します。

[構造一覧](drawing-inventory.ja.md)に、意味解読を行わないRustの入口と検証範囲を記載しています。
