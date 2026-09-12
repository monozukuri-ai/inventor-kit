# トレラント境界・有限 UV の開発記録

[English](tolerant-trims.md) | 日本語

このブランチは、対応する `cq-acis` の開発実装に追従します。
公開済みの `acis-core`／`acis-py-bridge`／`cq-acis` 0.3.2 には今回の追加を含みません。
パッケージの版と本番の依存固定は更新・公開していません。
共有依存の公開と明示的な依存更新後に、通常の registry-only CI を検証する必要があります。

共通 Rust パーサで、観測済み ASM 22700 の forward 明示 spline と subtype 参照の有限定義域を保持します。
変換器は確認済みのトレラント辺・coedge と、有限領域内のトリムを扱います。
同じ spline 定義を参照する保存済み次数1 UV 曲線は、追加の native ビューで取得します。
局所的なトレラント数値で、モデル本来の許容差を広げません。

対応する開発依存をインストールして、次のコマンドで再現できます。

```sh
python scripts/validate_components.py
python scripts/validate_topology.py
python scripts/validate_tolerant_trims.py
python scripts/validate_geometry.py
```

各スクリプトは固定 fixture の hash を確認し、詳細結果を `internal/reports/latest/` に保存します。
新しい検査では FTC07 2021 の回帰ファイルだけを使用し、有限曲面・境界・pcurve の source span を保持します。
全体の形状検査では holdout の区分と固定ソリッド量を別途維持します。
入力不足や assertion 失敗はエラーです。

ローカルの開発検証では、FTC07 のトレラント coedge 195件を辺へ変換し、
有限曲面8件、保存直線 pcurve ビュー153件を取得しました。
個別に有効な面として変換できる数は、258面中112面から192面に増えています。
曲面上の曲線との整合性が残るため、FTC07 全体の変換は引き続き拒否します。
有限の native 面8件も、面全体の変換にはまだ成功していません。
全コーパスの有効ソリッド変換数は10件のままです。

残項目は、局所的な辺・coedge の許容差の意味、inline curve、追加の保存 pcurve chart／版、
ベンダー比較です。今回の結果は現在の Model State、native 外観、リモート CI、公開配布物の検証ではありません。
