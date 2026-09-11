# ローカル部品・アセンブリ viewer

[English](viewer.md) | 日本語

追加機能の viewer は、対応する IPT の保存形状と IAM の保存配置、文書属性、保存プレビュー、
形状候補、読み込み診断を表示します。Python API と同じ限定パーサと
CadQuery 変換を使い、ローカルで実行します。

```sh
python -m pip install 'inventor-kit[viewer]'
python -m inventor_kit.viewer part.ipt
```

コマンドはローカル URL を出力し、ブラウザを開きます。閲覧中はプロセスを起動したままにし、
終了時は端末で Ctrl-C を押してください。タブを閉じるだけでは終了しません。
利用時の Node.js、Inventor、ネット接続は不要です。Windows は Ctrl-Break でも正常終了します。

配布 workflow は Linux x86_64、Windows x86_64、macOS arm64 / x86_64 のそれぞれで、
Python 3.11 / 3.12 の新規環境に viewer extra を導入します。IPT、保存配置を許可した IAM、
部分表示を明示許可した IAM のシーン・メッシュ配信、正常終了、一時データ削除を必須にしています。
フルのブラウザ試験は Linux Chromium のソフトウェア描画で実行します。
Windows/macOS のブラウザ描画と実機 GPU 性能は、サーバー試験の確認範囲に含めません。
必要な結果と artifact は[リリース手順](releasing.ja.md)を参照してください。

未リリースの版を試す場合は、成功した `Python distributions` workflow から対象 OS の wheel を
ダウンロードし、実際のファイル名を使って
`python -m pip install './inventor_kit-<version>-<abi>-<platform>.whl[viewer]'` で導入します。
上記の PyPI コマンドは公開済みの版を導入するため、その版には開発中の viewer の変更が
まだ含まれていない場合があります。

```sh
python -m inventor_kit.viewer part.ipt --no-browser --port 0
python -m inventor_kit.viewer part.ipt --quality fine
python -m inventor_kit.viewer part.ipt --candidate-id CANDIDATE_ID
python -m inventor_kit.viewer part.ipt --require-current-state
python -m inventor_kit.viewer drawing.idw --metadata-only
python -m inventor_kit.viewer assembly.iam
python -m inventor_kit.viewer assembly.iam --allow-unverified-state
python -m inventor_kit.viewer assembly.iam --search-root parts --allow-unverified-state --allow-partial
```

`--port 0` は空きポートを選びます。`--no-browser` はブラウザを開かず URL を出力します。
`--metadata-only` は viewer extra がなくても使え、Python の形状モジュールを import しません。
候補・現在状態の指定とは併用できません。候補 ID は同じ入力全体に結び付きます。
[API ガイド](api.ja.md)を参照してください。候補選択は起動時に行います。
別の入力や候補を確認するときは、パスや ID を指定してコマンドを起動し直します。
アセンブリ用の指定は `--metadata-only` や IPT の候補・現在状態指定とは併用できません。

IAM は既定で参照グラフと occurrence のツリーを表示し、形状モジュールを import しません。
保存配置を 3D 表示する場合は `--allow-unverified-state` を指定します。
これは現在の Model State、表示状態、代替部品の選択が未検証であることを了承する指定であり、
検証済みに変更するものではありません。必要な形状が揃わない場合はツリーと理由を残し、
`--allow-partial` も指定した場合に限り表示できる部分を描画します。
既存変換器の欠落だけでなく、メッシュ生成の失敗にも適用します。
`--allow-partial` には `--allow-unverified-state` が必要です。

参照先の探索ディレクトリは `--search-root` を繰り返して指定できます。
既存のオフライン resolver はルート文書のディレクトリと明示した探索先を検索し、再帰探索はしません。
参照 ID 不一致、同名衝突、循環、欠落、未対応配置は元の理由を保持します。
配置不明の部品もツリーに残し、未知の配置を原点へ置き換えて描画することはありません。
解決済みの部品・アセンブリ・別名パスのファイルが変更された場合は未確定の形状を拒否します。
resolver の対応範囲は[アセンブリ API](api.ja.md)を参照してください。

マウスで回転、パン、ズームできます。ツールバーには Fit、Isometric、Front、Top、Edges があります。
左の一覧でボディを選択するか、3D 上でダブルクリックすると選択枠が表示されます。
IAM は欠落部品とアセンブリのグループも含む階層ツリーを表示します。
チェックボックスで表示を切り替え、グループの操作は配下の部品に適用します。
**Isolate** は選択した occurrence またはグループだけを表示し、**Show all** は表示可能な全形状を戻します。
同じ定義の occurrence はメッシュとバッファの取得を共有しつつ、別々に選択できます。
これらは閲覧中の表示操作であり、保存された表示状態や Model State を変更しません。
色は viewer が割り当てた表示色で、
Inventor の外観は未解析です。

**Current Model State is unverified** の表示は、メッシュを描画できた場合も適用されます。
保存プレビューは別の保存状態を示す場合があり、3D 表示と区別します。
形状変換に失敗しても、取得済みの属性、プレビュー、候補、元の診断を残します。
IDW、IPN は現時点では文書情報のみで、図面描画やプレゼンテーションのアニメーションには対応しません。
[対応範囲](support.ja.md)を参照してください。

`--quality` は `draft`、`normal`（既定）、`fine` から選びます。
OCCT に渡す絶対 linear deflection は 0.3 / 0.1 / 0.03 mm、angular deflection は
0.3 / 0.1 / 0.05 rad です。これらは表示設定で、保証された寸法誤差ではありません。
全 B-rep 面に三角形があることを要求し、面が欠落した場合は部品のメッシュ表示を拒否します。
B-rep の辺は別途離散化します。CadQuery 変換器が拒否する曲線・曲面への対応は追加しません。

変換は spawn 方式の子プロセスで実行し、既定の期限は 120 秒です。`--timeout` で変更できます。
パーサの[文書単位の上限](api.ja.md#文書単位の上限)も適用します。
表示出力の上限は三角形 200 万個、メッシュバッファ 128 MiB、シーン JSON 16 MiB、
保存プレビュー 64 枚 / PNG 合計 32 MiB です。三角形数の上限は重複 occurrence の展開後にも適用します。
resolver の既定上限（文書 256 件、instance 10,000 件、深さ 32、入力ファイル合計 512 MiB、
ディレクトリエントリ 100,000 件）も引き継ぎます。OCCT の中間メモリ確保やプロセス RSS の上限を
保証するものではありません。期限超過や異常終了時は未確定の形状を破棄し、公開済み文書情報と occurrence 一覧を残します。
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
IAM は実データの Subassembly と、SampleBg の表示できる 5 部品・欠落 2 件を確認します。
合成試験で定義の共有、順序によって結果が異なる親子回転、形状自体の位置、未知配置、
元ファイル変更、一部のメッシュ生成失敗、選択の独立性を確認します。
holdout の IAM は未対応の native プロファイルとして拒否されることを維持します。

内部シーンの行列は行優先の 4×4、列ベクトルへの作用、単位 mm です。
定義メッシュはローカル形状を保持し、階層の各辺で local 配置を一度だけ適用します。
合成結果を保存済み world 行列と変換器の実際の配置に照合し、フロントエンドで local 回転を
renderer の quaternion に変換します。境界箱はカメラの表示範囲に使い、正確な CAD 寸法とは扱いません。

```sh
python scripts/smoke_distribution.py --wheel dist/inventor_kit-0.1.0-cp310-abi3-manylinux_2_34_x86_64.whl --viewer --browser
python scripts/smoke_distribution.py --sdist dist/inventor_kit-0.1.0.tar.gz --viewer --browser
```

wheel 名は実際にビルドしたファイル名に合わせてください。Windows/macOS のサーバー試験では
`--browser` を外します。`--python` で導入先の Python を選択できます。
checkout 外へインストールして配信と後片付けを確認し、`--browser` を付けた場合は、
導入した Python で Linux Chromium のブラウザ試験も実行します。
`--report <file.json>` は、全指定試験と Python の正常終了後に、選択した件数、依存版、
OS 種別・Python 版、入力配布物の SHA-256 を保存します。文書属性、参照パス、メッシュ、
プレビュー、詳細診断は含めません。
sdist は生成済みアセットとソース・lockfile を含み、Python wheel の再ビルドには Node を必要としません。
配布条件は[リリース手順](releasing.ja.md)を参照してください。
