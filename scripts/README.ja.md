# 公開開発ツール

[English](README.md) | 日本語

Python 3.11 以降を使用します。通常の再現手順は [開発ガイド](../docs/development.ja.md)にあります。
詳細結果の既定保存先は `internal/reports/latest/` です。内部資料がない clone でも実行できます。

| ツール | 役割 |
| --- | --- |
| `fetch_public_samples.py`, `fetch_assembly_samples.py` | 固定した出典から fixture を取得 |
| `check_corpus.py`, `corpus_manifest.py`, `run_tests.py` | hash と split の検査、skip を許さないテスト |
| `validate_public_samples.py` | コンテナ・共通モデル・形状・任意オラクルの段階別検証 |
| `validate_document_samples.py`, `validate_candidate_samples.py` | 属性・保存候補・出典の検証 |
| `validate_geometry.py`, `validate_components.py`, `validate_topology.py` | 固定形状量と曲線・曲面・トポロジの検証 |
| `validate_tolerant_trims.py` | [トレラント・有限 UV 対応](../docs/tolerant-trims.ja.md)と固定した元データの要素を照合 |
| `validate_assembly.py` | IAM 参照・配置・欠落・STEP の回帰と保留検証 |
| `benchmark_parser.py`, `seed_fuzz_corpus.py`, `run_fuzz.py` | 固定入力の計測と bounded ASan fuzz |
| `capture_vendor_oracle.py`, `capture_model_states.py` | 任意の Windows / Autodesk オラクル取得 |
| `oracle_contract.py`, `compare_state_candidates.py`, `ezdxf_oracle.py` | オラクル契約と比較 |
| `check_dependencies.py`, `check_distribution.py`, `smoke_distribution.py`, `check_release.py` | 依存・配布物・隔離インストール・公開ゲート |
| `check_public_tree.py`, `summarize_validation.py` | 公開リンクの検査、公開用集計の生成 |

一時的な依存 patch、初期のバイト列探索、作業経緯の調査コードは `internal/scripts/` に保存します。
公開 CI はそれらのコードを呼び出しません。
