# 開発と公開ディレクトリ

[English](development.md) | 日本語

## ビルドと検証

ライブラリと開発スクリプトは Python 3.11 以降を使用します。
Rust は CI と同じ 1.93、maturin は 1.11.5 を検証用の基準にします。
共通依存 `acis-core` / `acis-py-bridge` 0.3.2 と `cq-acis>=0.3.2,<0.4` が
公開レジストリから取得できることが、通常の開発・CI の前提です。
以前の開発用 `.cargo/config.toml` で bridge を差し替えている場合は、設定を
`internal/` へ退避してからビルドしてください。`scripts/check_dependencies.py` は
core と bridge がそれぞれ一つの公開 crate として解決されることを確認します。

```sh
python3.11 -m venv .venv
. .venv/bin/activate
python -m pip install 'maturin[patchelf]==1.11.5'
maturin develop --release --locked --extras validation
python scripts/check_dependencies.py
python scripts/check_public_tree.py
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
python scripts/fetch_public_samples.py
python scripts/fetch_assembly_samples.py
python scripts/run_tests.py
```

Windows の仮想環境では `.venv/Scripts/python.exe` を使用します。
fixture の取得は明示的なネット操作です。取得後の通常パーサと検証はオフラインで実行できます。
`run_tests.py` はマニフェスト全件の存在・サイズ・SHA-256 を確認し、skip を失敗にします。

```sh
python scripts/validate_public_samples.py
python scripts/validate_document_samples.py
python scripts/validate_candidate_samples.py
python scripts/validate_geometry.py
python scripts/validate_components.py
python scripts/validate_topology.py
python scripts/validate_assembly.py
python scripts/benchmark_parser.py
```

生成された詳細結果は既定で `internal/reports/latest/` に書き込みます。
初回実行に内部資料は不要です。形状の比較値は公開の
[geometry-baseline.json](../tests/data/geometry-baseline.json)、計測入力は
[cases.json](../benchmarks/cases.json) を使用します。

## Fuzz

```sh
rustup toolchain install nightly-2026-02-02 --profile minimal
cargo install cargo-fuzz --version 0.13.2 --locked
cargo fetch --manifest-path fuzz/Cargo.toml --locked
python scripts/seed_fuzz_corpus.py
python scripts/run_fuzz.py --seconds 30
```

`fuzz/` は独立 lockfile を持ちます。回帰用 seed と合成入力を使い、保留入力は使いません。
二つの ASan ターゲットは CFB 文書と RSe/UFRx/SAB ストリームを検査します。
短時間の smoke は網羅性の証明ではありません。

## 公開対象の役割

| パス | 保存するもの |
| --- | --- |
| `python/`, `crates/` | ライブラリ実装と Rust テスト |
| `docs/`, `README.md`, `README.ja.md` | 英語・日本語の API、対応範囲、検証・配布手順 |
| `scripts/` | 再現可能なビルド・検証・配布ツール |
| `tests/`, `schemas/`, `fixtures/*.json` | テスト、公開契約、固定した入力の出典と hash |
| `benchmarks/` | 固定入力と計測方法 |
| `reports/` | 確認済みの集計だけを含む公開サマリー |
| `internal/` | 作業日誌、旧計画、調査用コード、ホスト情報、詳細結果 |

`internal/` は Git と wheel/sdist の対象外です。外部 CAD の実ファイル、fuzz の
corpus/artifacts、ローカル Cargo 設定も公開パッケージに含めません。
公開コードは内部ファイルを入力として要求しません。任意に指定した比較結果やオラクルは例外です。
`.gitignore`、Git archive の `export-ignore`、配布物検査と CI の公開リンク検査で境界を維持します。

ローカルの開発用依存で通った結果を、公開依存だけでのビルド成功と扱わないでください。
リリースは [配布手順](releasing.ja.md)の条件を別途満たす必要があります。

## ドキュメントの言語

`README.md` と公開ガイドは英語を標準とします。日本語版は同じディレクトリの
`.ja.md` に置き、各ページ先頭で相互にリンクしてください。コマンド、依存バージョン、
API の動作、対応範囲や検証結果を変更したときは、両言語を同じ変更で更新します。
日本語ガイド内のリンクは、日本語版があればそちらを参照します。
API 識別子とコマンドは両言語で共通です。
リンク変更後は `python scripts/check_public_tree.py` を実行してください。
