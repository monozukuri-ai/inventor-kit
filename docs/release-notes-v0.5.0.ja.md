# inventor-kit v0.5.0

[English](release-notes-v0.5.0.md) | 日本語

v0.5.0では、既存のIPT/IAM処理に加えて、IDWの保存図面表示・レポート・オフラインSVG保存を追加します。
図面は実験的な部分対応です。

## 図面の操作

- `read_drawing_file`で保存シート・ビュー・対応曲線・文字・スタイル・埋め込み画像を読み取れます。
  対象は観測済みのschema31 / Meta8、major23（zlib）とmajor31（zstd）です。
- IDWを通常のViewer起動で開き、シート選択・パンと拡大・文字検索・出典確認ができます。
  IDW表示にはPythonの`viewer` extraは不要です。従来の`--experimental-drawing`も引き続き受理します。
- CLIでシート一覧・JSONレポートを取得し、複数図面のレポートをJSONLで処理できます。
  入力ごとのタイムアウトと分類済み終了コードを利用できます。
- CLI・Python API・Viewerの共通描画処理で、選択シートをSVG保存できます。
  画像は埋め込み、付属JSONに入出力ハッシュ・原文・出典・省略理由を記録します。
  既存ファイルは上書きせず、SVG保存には部分出力への明示的な同意が必要です。

## 描画とOS対応の修正

- 円・楕円弧をSVGの弧として描画します。ほぼ直線となる楕円は、偏差上限を持つ線分近似として記録します。
- Inventorで保存した38制御ケースから、major31の標準15線種の公称配列、レイヤー継承、
  観測済みの線幅連動を解読しました。レイヤー・bindingの尺度は観測済みの1に限定します。
  個別指定の保存配列には尺度が含まれるため、二重に拡大しません。
- macOSを含む日本語・直径記号のフォント代替を改善しました。
- WindowsでViewerの状態ファイル置換時に起きる一時的な共有・アクセスエラーを再試行します。
  継続するエラーは失敗として扱います。

## インストールと配布検証

```sh
python -m pip install --upgrade inventor-kit==0.5.0
```

Python 3.11以降が必要です。依存契約は`cq-acis>=0.3.8,<0.4`を維持し、
既存のIPT/IAMのbody選択・STEP処理も利用できます。
ライセンス条件は[ライセンスガイド](license.ja.md)のとおりです。

配布workflowはLinux x86_64・Windows x86_64・macOS Intel・Apple Siliconの4 wheelとsdistを要求します。
Python 3.11/3.12で計9件の隔離導入を行い、sdistは独立した環境で再ビルドします。
各OSのChromiumで、導入済みwheelから実5シートとフォント・破線の合成1ケースを描画します。
SVG表示の検査であり、実機GPUやSafariの認定ではありません。

## 制限

- 図面は`experimental_partial`・`qualified=False`・`complete=False`を維持します。
  現在状態と物理単位は未検証です。SVG座標はmm・寸法測定・実寸印刷を保証しません。
  mm単位を要求する`render_sheet`は引き続き対象プロファイルを拒否します。
- 保存座標とAPI値の一致は4つの単位制御入力だけの結果です。
  native PDFは0.001 mmの比較基準を満たしておらず、一般IDWの実寸保証には広げません。
- native PDFの破線周期調整と曲線位相は再現しません。SVGは公称配列を保持し、
  `dash_phase_and_fit_unverified`を報告します。major23の一般レイヤー線種・カスタム`.lin`・
  未観測の尺度は未認定です。
- 未対応要素は省略理由を残します。nativeフォント一致・注記の完全な意味情報・モデルの再投影は提供しません。
  SVGへフォントは埋め込まず、major23のスプライン表示も近似です。
- 部分対応のIDWレポート・SVG保存は、保存成功時もCLI終了コード**2**を返します。

API契約・処理上限・出力の詳細は[図面ガイド](drawing.ja.md)と[CLIの例](cli.ja.md)を参照してください。
