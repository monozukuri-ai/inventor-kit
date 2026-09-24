# リリース手順

[English](releasing.md) | 日本語

## 依存契約

Rust は `acis-core=0.3.8` と `acis-py-bridge=0.3.8`、Python は
`cq-acis>=0.3.8,<0.4` と共通モデル API 2 を必要とします。
Python の必要バージョンは cq-acis に合わせて 3.11 以降です。
必要な依存が公開され、Cargo.lock が bridge と core の registry source/checksum を
保持していることが前提です。ローカル path patch や未公開 wheel による結果では代替しません。
この資料は公開済み・CI 通過済みという宣言ではありません。

## 手順

0.3.0 で [ライセンスを移行](license.ja.md)します。旧 MIT 表示と第三者条件を維持してください。
新しい LICENSE を導入した最初のコミットをソースの切替記録とし、その完全な SHA と公開日、
直前の公開 MIT コミットをリリースノートへ記録します。既存タグ・配布物を書き換えません。

初回公開前に会社が対象権利を許諾できる根拠を記録し、商用窓口を確認します。
商用案内ページは締結済み契約ではありません。新規の外部貢献を取り込む前に、
CLA への明示的な同意を記録してください。

ライセンス文書を変更したら `python scripts/sync_license_notices.py` と通常の Viewer ビルドを
実行します。Cargo 依存変更時は、先に `cargo fetch --locked` と
`python scripts/sync_license_notices.py --refresh-rust` を実行し、実際の同梱物に対して
通知一覧と複合 SPDX 式を確認します。一覧では対応環境に含まれない UEFI 専用依存を明示的に除外します。

ビルド前に `python scripts/check_license.py` を通してください。
`cargo package -p inventor-core --list` と実際の `.crate` も確認し、単体の LICENSE が
生成した通知と一致することを確認します。License-File は wheel の `.dist-info/licenses/`
または sdist ルートにある一致した本文を指す必要があります。別の場所に同名ファイルがあるだけでは
合格にしません。商用案内、旧 MIT と第三者通知を、各配布物と Viewer 単体へ保持してください。

1. 公開依存だけを使うクリーンな checkout で `scripts/check_dependencies.py`、
   通常 CI、コーパスと保留検証を通す。`.cargo/config.toml` を持ち込まない。
2. Python・二つの Rust パッケージ・Viewerの版をそろえる。`Cargo.lock`・`fuzz/Cargo.lock`・
   `uv.lock`・`viewer/package-lock.json`を更新し、Viewerアセットを再生成する。
   `scripts/check_release.py --tag v<version>`・`scripts/check_license.py`・
   `scripts/check_viewer_assets.py`を通す。
3. `Python distributions` workflow を手動実行する。4 種類の wheel と sdist が
   viewer 配布検証レポートとともに artifact に保存される。手動実行でも `qualify` ジョブの
   通過を必須とする。手動実行は PyPI に公開しない。
4. 各 artifact をダウンロードし、hash・ライセンス・配布内容・実インストール結果を確認する。
   `distribution-*` は配布物、`viewer-platform-*` は選択した導入結果の集計だけを含む。
5. PyPI の Trusted Publisher を repository `monozukuri-ai/inventor-kit`、
   workflow `release.yml`、environment `pypi` に設定・確認する。
6. パッケージ版と一致する `v<version>` の GitHub Release を公開する。
   全ゲート通過後に Trusted Publishing で配布する。

## CI の配布対象と検査

| artifact | ビルド対象 | インストール確認 |
| --- | --- | --- |
| ABI3 wheel | Linux x86_64 / manylinux2014 | Python 3.11 / 3.12 |
| ABI3 wheel | Windows x86_64 | Python 3.11 / 3.12 |
| ABI3 wheel | macOS arm64 | Python 3.11 / 3.12 |
| ABI3 wheel | macOS x86_64 | Python 3.11 / 3.12 |
| sdist | ソース一式 | 別ディレクトリ・新規 target で locked/offline 再ビルド |

wheel の名前・版・ABI/platform tag、CRC、RECORD の hash/size、必要なモジュールと
ライセンスを照合します。隔離環境で属性読込、形状変換、アセンブリ STEP 往復を行い、
子プロセスの正常終了を確認します。固定した FTC07 で有限曲面の参照、保存 UV 曲線、
トレラント coedge を検査し、258 面の有効ソリッドへの全体変換を確認します。
FTC06（2021/2024）は 146 面の主ソリッドが有効で、開いた補助 body が二つ残るため
全体変換は拒否することを確認します。CTC04（2021）は 368 面全てを含む、閉じた
有効ソリッドへの変換を必須とします。
各 OS 種別・Python 版で `wheel[viewer]` を導入して
`pip check` を行い、`--no-browser` で起動します。部品・アセンブリ・明示許可した部分部品・部分
アセンブリの4ケースでシーンとメッシュバッファを取得します。major31 IDWの通常起動・互換flag起動と、
major23の4シートも検査し、リソースと共通SVG出力のハッシュ、単位未検証・部分対応の状態を確認します。
全7ケースで正常終了と一時データ削除を要求します。
sdist は Node なしで再ビルドし、生成した wheel を検査したうえで Linux / Python 3.11 に
viewer extra とともに導入します。
インストール検査は依存先も wheel に限定します。`cp310-abi3` は拡張の ABI 下限で、
パッケージの利用可能な Python バージョンは `Requires-Python: >=3.11` で制限します。

FTC07の体積検査は、3段階の許容誤差による適応積分で推定誤差と収束を確認し、比較の相対許容誤差
`1e-10` を維持します。既定積分の値も診断ログに残します。FTC06の部分body出力では、
検証済みSTEPの付属JSONに除外した2bodyが残ることも必須です。

```sh
maturin build --release --locked --out dist
maturin sdist --out dist
python scripts/check_distribution.py dist/*.whl dist/*.tar.gz
python scripts/smoke_distribution.py --wheel dist/*.whl --viewer --report qualification/wheel.json
python scripts/smoke_distribution.py --sdist dist/*.tar.gz --viewer --report qualification/sdist.json
```

前提の Linux CI は、導入した wheel と別途再ビルドした sdist の両方で Chromium E2E を
必須にします。同梱アセットは、固定した lockfile と Node の基準版による `npm ci`・
再生成の結果に照合します。合成入力の作成は`smoke_distribution.py`を起動したPythonで行うため、
この環境には`olefile`を含む`validation` extraが必要です。スクリプトが設定する
`VIEWER_FIXTURE_PYTHON`は入力作成専用で、製品の実行にはクリーンな`wheel[viewer]`環境の
`VIEWER_PYTHON`を使います。ブラウザ試験はソフトウェア描画であり、実機 GPUやInventorの現在のModel Stateの検証ではありません。
配布workflowでは、4プラットフォームのPython 3.12 cold installに対して追加の
Chromium SVG検査を要求します。公開fixtureの実5シートと合成1ケースを表示し、
描画範囲・画像数・日本語/直径記号の使用フォントを確認します。PNGとブラウザー版を
`drawing-browser-*` artifactへ保存します。これはSVGの表示検証で、各OSの3Dブラウザー操作、
Safari/WebKit、nativeフォント一致、実寸印刷の認定ではありません。
シナリオとローカル実行コマンドは [viewer ガイド](viewer.ja.md)を参照してください。
依存 wheel の不足や実行失敗は配布検証を失敗させます。未検証 OS を対応済みと扱わないでください。

`qualify` ジョブは全 5 配布物と、4 種類の wheel × Python 3.11 / 3.12、Linux の sdist 再ビルド
× Python 3.11 の計 9 レポートを要求します。実際の配布物の SHA-256 と対象 OS が一致し、
公開依存によって全シナリオと正常終了が成功している必要があります。
不足・重複・古い hash・失敗結果があると停止します。ダウンロード後の再検査は次の形です。

```sh
python scripts/check_release.py --dist dist --viewer-reports qualification
```

`dist/` には 5 配布物だけ、`qualification/` には 9 件の OS 別レポートだけを置きます。
Linux CI の `validation-report` artifact は、別途選択したコーパスと viewer のブラウザ検証の
集計を含みます。公開集計は件数、OS 種別・Python・依存版、配布物の hash に限定し、
文書属性、参照パス、メッシュ、プレビュー、詳細診断、生ログを含めません。
シーンの現在状態は引き続き未検証です。

`internal/`、外部 CAD、実行ログ、fuzz corpus、ローカル Cargo 設定を配布しません。
sdist には英語・日本語の README と公開 `docs/`、ビルド入力を含めます。
wheel には API・capabilities・ライセンスを含め、パッケージ説明には英語版 README を使います。
リポジトリ用の取得・検証スクリプトと fixture 索引はソースリポジトリから使用します。

Release workflow は全 4 プラットフォームと sdist の集合を検査し、同じ版の既存 PyPI
ファイルがあれば SHA-256 一致を要求します。異なる内容や 404 以外の照合失敗は停止します。
再実行時の `skip-existing` もこの検査後に限ります。
