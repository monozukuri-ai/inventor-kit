# ローカル部品 viewer

[English](viewer.md) | 日本語

追加機能の viewer は、対応する IPT の保存形状、文書属性、保存プレビュー、
形状候補、読み込み診断を表示します。Python API と同じ限定パーサと
CadQuery 変換を使い、ローカルで実行します。

```sh
python -m pip install 'inventor-kit[viewer]'
python -m inventor_kit.viewer part.ipt
```

コマンドはローカル URL を出力し、ブラウザを開きます。閲覧中はプロセスを起動したままにし、
終了時は端末で Ctrl-C を押してください。タブを閉じるだけでは終了しません。
利用時の Node.js、Inventor、ネット接続は不要です。初版の viewer は Linux を検証対象とし、
他 OS のインストール・ブラウザ確認は、既存パーサの配布確認とは別に扱います。

```sh
python -m inventor_kit.viewer part.ipt --no-browser --port 0
python -m inventor_kit.viewer part.ipt --quality fine
python -m inventor_kit.viewer part.ipt --candidate-id CANDIDATE_ID
python -m inventor_kit.viewer part.ipt --require-current-state
python -m inventor_kit.viewer drawing.idw --metadata-only
```

`--port 0` は空きポートを選びます。`--no-browser` はブラウザを開かず URL を出力します。
`--metadata-only` は viewer extra がなくても使え、Python の形状モジュールを import しません。
候補・現在状態の指定とは併用できません。候補 ID は同じ入力全体に結び付きます。
[API ガイド](api.ja.md)を参照してください。候補選択は起動時に行います。
別の入力や候補を確認するときは、パスや ID を指定してコマンドを起動し直します。

マウスで回転、パン、ズームできます。ツールバーには Fit、Isometric、Front、Top、Edges があります。
左の一覧でボディを選択するか、3D 上でダブルクリックすると選択枠が表示されます。
チェックボックスでボディの表示を切り替えます。色は viewer が割り当てた表示色で、
Inventor の外観は未解析です。

**Current Model State is unverified** の表示は、メッシュを描画できた場合も適用されます。
保存プレビューは別の保存状態を示す場合があり、3D 表示と区別します。
形状変換に失敗しても、取得済みの属性、プレビュー、候補、元の診断を残します。
IAM、IDW、IPN は現時点では文書情報のみで、アセンブリ配置や図面描画は初版に含みません。
[対応範囲](support.ja.md)を参照してください。

`--quality` は `draft`、`normal`（既定）、`fine` から選びます。
OCCT に渡す絶対 linear deflection は 0.3 / 0.1 / 0.03 mm、angular deflection は
0.3 / 0.1 / 0.05 rad です。これらは表示設定で、保証された寸法誤差ではありません。
全 B-rep 面に三角形があることを要求し、面が欠落した場合は部品のメッシュ表示を拒否します。
B-rep の辺は別途離散化します。CadQuery 変換器が拒否する曲線・曲面への対応は追加しません。

変換は spawn 方式の子プロセスで実行し、既定の期限は 120 秒です。`--timeout` で変更できます。
パーサの[文書単位の上限](api.ja.md#文書単位の上限)も適用します。
表示出力の上限は三角形 200 万個、メッシュバッファ 128 MiB、シーン JSON 16 MiB、
保存プレビュー 64 枚 / PNG 合計 32 MiB です。OCCT の中間メモリ確保やプロセス RSS の上限を
保証するものではありません。期限超過や異常終了時は未確定の形状を破棄し、公開済み文書情報を残します。
viewer の正常起動はモデルの完全性を示しません。読み込み失敗は UI に表示し、
不正な引数や起動失敗は CLI の非ゼロ終了とします。

サーバーは `127.0.0.1` だけに bind し、同梱アセットと現在のセッションの生成資源を配信します。
アップロードや任意ファイルを開く API は提供しません。正常終了時にセッションの一時データを削除します。
インストールと最初の fixture 取得にはネット接続が必要ですが、導入済みパッケージの閲覧には不要です。

開発用のフロントエンドは `viewer/`、生成済みアセットは
`python/inventor_kit/viewer/static/` に置き、Git に含めます。
統合試験の前に Python の viewer / validation extra を開発環境に導入してください。
アセットビルドの基準は Node 22.18.0 / npm 10.9.3 です。

```sh
npm ci --prefix viewer
npm run build --prefix viewer
python scripts/check_viewer_assets.py
python scripts/run_viewer_tests.py
cd viewer
npx playwright install chromium
npm test
```

viewer の Python 統合試験は基本テストから分けて `viewer_tests/` に置きます。
必要な依存・fixture の不足と skip は失敗にします。ブラウザ試験は外部リクエストを遮断し、
実部品、未対応形状、現在状態指定による拒否、文書情報のみの表示、破損入力を確認します。
スクリーンショットはローカル試験結果で、Inventor との照合の証拠ではありません。
メッシュ試験では解析式を持つ円筒管も使い、穴、符号付き体積、三角形の向きを確認します。

```sh
python scripts/smoke_distribution.py --wheel dist/inventor_kit-0.1.0-cp310-abi3-manylinux_2_34_x86_64.whl --viewer --browser
python scripts/smoke_distribution.py --sdist dist/inventor_kit-0.1.0.tar.gz --viewer --browser
```

wheel 名は実際にビルドしたファイル名に合わせてください。Linux のこれらの確認では、
checkout 外へインストールして配信と正常終了を確認し、導入した Python でブラウザ試験も実行します。
sdist は生成済みアセットとソース・lockfile を含み、Python wheel の再ビルドには Node を必要としません。
既存の配布条件は[リリース手順](releasing.ja.md)を参照してください。
