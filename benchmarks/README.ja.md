# ベンチマーク

[English](README.md) | 日本語

[cases.json](cases.json) は、公開回帰サンプルから固定した 5 件の処理・パス・SHA-256 を定義します。
CAD 本体は [取得手順](../docs/development.ja.md)で取得し、ここには同梱しません。

```sh
python scripts/benchmark_parser.py --output internal/reports/latest/benchmark.json
```

各ケースを別プロセスで初回 + 7 回実行し、cold 時間、warm 中央値・最大値、
プロセス peak RSS を記録します。部品 API は入力ファイルの読み込みと inventor-kit import を
計時外にし、アセンブリ API 内のファイル解決は計時します。
形状モジュールの初回 import は cold 時間に含まれます。
RSS は Python と依存ライブラリを含む最大値です。Linux は KiB、macOS は bytes から
統一し、Windows は取得未対応として null を返します。

数値はホストごとの観測値で、速度・メモリ使用量の汎用保証ではありません。
ホスト固有の目標は `--goals <JSON>` で任意指定できます。公開ケースは
ローカルの目標ファイルや過去の計測結果に依存しません。
詳細結果・ホスト情報・目標は `internal/benchmarks/` または `internal/reports/` に保存してください。
