# リリース手順

[English](releasing.md) | 日本語

## 依存契約

Rust は `acis-core=0.3.2` と `acis-py-bridge=0.3.2`、Python は
`cq-acis>=0.3.2,<0.4` と共通モデル API 2 を必要とします。
Python の必要バージョンは cq-acis に合わせて 3.11 以降です。
必要な依存が公開され、Cargo.lock が bridge と core の registry source/checksum を
保持していることが前提です。ローカル path patch や未公開 wheel による結果では代替しません。
この資料は公開済み・CI 通過済みという宣言ではありません。

## 手順

1. 公開依存だけを使うクリーンな checkout で `scripts/check_dependencies.py`、
   通常 CI、コーパスと保留検証を通す。`.cargo/config.toml` を持ち込まない。
2. Python と二つの Rust パッケージの版をそろえる。`scripts/check_release.py` が版を検査する。
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
子プロセスの正常終了を確認します。各 OS・Python 版で `wheel[viewer]` を導入して
`pip check` を行い、`--no-browser` で起動します。部品・アセンブリ・明示許可した部分
アセンブリのシーンとメッシュバッファを取得し、正常終了と一時データ削除を要求します。
sdist は Node なしで再ビルドし、生成した wheel を検査したうえで Linux / Python 3.11 に
viewer extra とともに導入します。
インストール検査は依存先も wheel に限定します。`cp310-abi3` は拡張の ABI 下限で、
パッケージの利用可能な Python バージョンは `Requires-Python: >=3.11` で制限します。

```sh
maturin build --release --locked --out dist
maturin sdist --out dist
python scripts/check_distribution.py dist/*.whl dist/*.tar.gz
python scripts/smoke_distribution.py --wheel dist/*.whl --viewer --report qualification/wheel.json
python scripts/smoke_distribution.py --sdist dist/*.tar.gz --viewer --report qualification/sdist.json
```

前提の Linux CI は、導入した wheel と別途再ビルドした sdist の両方で Chromium E2E を
必須にします。同梱アセットは、固定した lockfile と Node の基準版による `npm ci`・
再生成の結果に照合します。ブラウザ試験はソフトウェア描画であり、Windows/macOS の
ブラウザ描画、実機 GPU、Inventor の現在の Model State の検証ではありません。
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
集計を含みます。公開集計は件数、OS・Python・依存版、配布物の hash に限定し、
文書属性、参照パス、メッシュ、プレビュー、詳細診断、生ログを含めません。
シーンの現在状態は引き続き未検証です。

`internal/`、外部 CAD、実行ログ、fuzz corpus、ローカル Cargo 設定を配布しません。
sdist には英語・日本語の README と公開 `docs/`、ビルド入力を含めます。
wheel には API・capabilities・ライセンスを含め、パッケージ説明には英語版 README を使います。
リポジトリ用の取得・検証スクリプトと fixture 索引はソースリポジトリから使用します。

Release workflow は全 4 プラットフォームと sdist の集合を検査し、同じ版の既存 PyPI
ファイルがあれば SHA-256 一致を要求します。異なる内容や 404 以外の照合失敗は停止します。
再実行時の `skip-existing` もこの検査後に限ります。
