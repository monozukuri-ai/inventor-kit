# 制御した差分IDWの作成手順

[English](drawing-specimens.md) · [収集・検証の契約](drawing-validation.ja.md)

これは作成手順であり、**取得済みのネイティブfixtureではありません**。
現時点でWindows環境は利用できません。合成JSONや公開サンプルでは、この実験の代用になりません。

最初はInventor年版/build、テンプレート、locale、フォント環境を一つに固定します。
テンプレートhash、provider/build、locale、database/表示単位、出力設定、フォントファイルの版・hashを残します。
別年版での保存も同じregression系列に含めます。予約済みのrim/vise holdoutを作成・調整に使いません。

A4横（29.7 × 21 cm）、名前 `Sheet A`、枠・表題欄・モデル参照なしの図面を作り、
保存・終了・再オープンして `blank.idw` とします。各行は指定のベースから一つだけ変更して別名保存します。
下表の数値は作成時の指定値であり、取得済みの期待値ではありません。APIが返した実測値を別に記録します。

| 保存名 / ベース | 制御する変更 | 独立して取得する証拠 |
| --- | --- | --- |
| `resave.idw` / blank | 編集なしで再保存 | hash、属性・streamのノイズ。byte一致を期待しない |
| `sheet-name.idw` / blank | 名前を `図面 A` に変更 | 名前と順序 |
| `sheet-portrait.idw` / blank | A4縦へ変更 | 幅・高さ・縦横 |
| `sheet-a3.idw` / blank | A3横へ変更 | 42.0 × 29.7 cmの観測値 |
| `two-sheets.idw` / blank | A4の `Sheet B` を追加 | 名前・寸法・順序 |
| `reordered.idw` / two-sheets | BをAより前に移動 | 順序だけの差分 |
| `line.idw` / blank | sheet sketchに (2,3) → (8,3) cmの線 | ローカル座標と紙面への写像 |
| `line-length.idw` / line | 終点を (10,3) に変更 | 終点差分 |
| `line-position.idw` / line | (1,2) cm平行移動 | 配置差分 |
| `circle.idw` / blank | 中心 (6,6)、半径2 cmの円 | 中心・半径 |
| `arc.idw` / blank | 中心 (6,6)、半径2、開始0、反時計回りπ/2 | 端点・有向角 |
| `text-ascii.idw` / blank | 一般注記 `ABC 123`、(2,10)、文字高0.35 cm | 原文・位置・style・font |
| `text-japanese.idw` / text-ascii | 文字列を `図面 日本語 ⌀10` に変更 | Unicodeと字形 |
| `text-multiline.idw` / text-ascii | `ABC` と `123` を実改行で区切る | 原文・改行・範囲 |
| `text-rotated.idw` / text-ascii | π/2回転 | 回転と画像上の配置 |
| `base-view.idw` / blank | 別保存した20×10×5 mmの直方体の正面図を (10,10) cm、尺度1で追加 | 参照hash・配置・投影辺 |
| `projected-view.idw` / base-view | 上方に投影図を一つ追加 | view所属・配置・辺 |
| `view-scale.idw` / base-view | 尺度0.5へ変更 | 尺度と紙面幾何 |
| `view-rotation.idw` / base-view | π/2回転 | 回転と幾何 |
| `hidden-lines.idw` / base-view | 隠線表示を有効化 | 可視性・隠線属性。穴が必要なら別のベースを明示 |
| `linear-dimension.idw` / base-view | 20 mm辺の寸法を一つ追加 | 表示文字・位置・寸法線の画像 |
| `diameter-dimension.idw` / circle | 径寸法を一つ追加 | 表示文字と記号 |
| `leader.idw` / base-view | 引出線注記を一つ追加 | JSONの取得範囲外を明示し、独立PDF/PNGを取得 |
| `title-block.idw` / blank | prompted/property文字を持つ表題欄を追加 | 評価済み文字と表示配置 |
| `parts-list.idw` / assembly base | 直方体2個の別IAMと基本図に部品表を追加 | IAM/IPT hash、セル値と配置 |
| `missing-reference.idw` / base-view | コピーしたprojectからIPTを外す | 参照欠落と保存表示の残存・劣化 |
| `deferred.idw` / base-view | 参照直方体の長さを変え、図面更新を保留 | IDW/IPT hash、更新状態、保存/起動後の差 |
| `without-preview.idw` / blank | 対応していればpreview保存設定だけ変更 | 図面データと独立したthumbnailの有無 |

部品表ケースは、表を入れる前のIAM図面をベースとして保存します。
参照欠落・更新保留・previewケースでは環境の差も実験変数なので、前後の入力をすべて保持します。
APIやUIが拒否した場合（sketch円への径寸法など）は結果を記録し、別の円形部品ベースを明示して用意します。
実験条件を暗黙に置き換えません。

各ケースの手順:

1. IDWと参照を保存して終了し、全ファイルをhash化します。作成時の指定と実測を分けます。
   実ファイル取得後にcanonical manifestへ登録し、仮のhashで埋めません。
2. 独立コピーで収集し、JSON・provider/build・collector hashを保持します。
   少なくとも空・線・一般注記・基本図についてApprenticeとInventorを比較し、providerを適格化します。
3. **同じ状態であると記録できるセッション**から各sheetをPDFへ独立出力し、
   固定renderer/versionで150 DPIのPNGに変換します。font、ページ寸法・縦横、アンチエイリアス、source hash、sheet番号を記録します。
   thumbnailのスクリーンショットを紙面全体の比較画像には使いません。
4. 起動・出力で対象保存状態が変化していないことを確認し、更新・移行・参照更新保留の記録を残します。
   不明ならunknownのまま保持し、更新後の結果は別の入力識別で管理します。
5. PDF/PNGのhash・bytes・セッション出典を `visual_artifacts` に付けます。
   取得ゲートにはsheetごとのPNGを用意し、PDFを補助資料として保持します。
   PDFを画像比較へ使う場合はラスタライズDPIを記録します。
6. 契約、入力・画像hash、照会の完全性をオフライン検査します。未対応要素は不足として保持します。
   少なくとも一つの保存プロファイルの空・線・文字で、紙面座標・表示文字・全紙面画像を独立に確認するまで、段階1を完了としません。

現収集処理では引出線の図形、表題欄の幾何、表の配置が不足します。
これらは独立出力を確保し、完全な意味情報の取得を主張する前に収集範囲を拡張してください。
画像位置合わせと数値許容差は制御したregression図面で決め、holdoutの受入評価前に固定します。
