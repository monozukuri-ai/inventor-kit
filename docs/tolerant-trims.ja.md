# トレラント境界・有限 UV

[English](tolerant-trims.md) | 日本語

今回の追加には `acis-core` / `acis-py-bridge` 0.3.3 と
`cq-acis>=0.3.3,<0.4` が必要です。通常ビルドと fuzz の依存グラフでは、
Rust crate を crates.io の版に固定しています。変換器と保存 pcurve ビューには
対応する Python 実装も必要なため、Python 側の最低バージョンも更新しています。
共通モデル API 2 は変わりません。

共通 Rust パーサで、観測済み ASM 22700 の forward 明示 spline と subtype 参照の有限定義域を保持します。
変換器は確認済みのトレラント辺・coedge と、有限領域内のトリムを扱います。
同じ spline 定義を参照する保存済み次数1 UV 曲線は、追加の native ビューで取得します。
モデル本来の許容差と有限 UV の範囲は維持します。

対応する依存をインストールして、次のコマンドで再現できます。

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

## 未公開の追加対応

対応する cq-acis 開発ソースでは、次数 1／3 の保存 UV spline と、曲線の反転方向・支持曲面の法線方向を扱います。
`NativeModel.spline_surface_pcurve()` は保存方向の knot と制御点、区間、元レコードと支持定義を保持します。
0.3.3 の直線ビューと共有モデル API 2 は変わりません。

同一の支持定義を持つ保存 UV 曲線と、対応済みの null-inline トレラント coedge／edge の組合せに限り、
辺の保存数値を mm 換算した値に元のモデル許容差を加え、2D／3D の偏差の上限として使います。
ASM 22700／内部 22601 の観測に基づく解釈で、ベンダーによるフィールド定義の確認は未実施です。
変換器は適用した辺、元の数値、配置倍率、上限と実測偏差を `source_edge_tolerances` に記録します。
元の頂点座標、3D／UV 制御点、モデル許容差、有限 UV 範囲は変更しません。
保存 fit tolerance と意味未確定の頂点フィールドは上限を広げる根拠にしません。

2026-09-13 のローカル検証では、FTC07 の個別に有効な面が **192 / 258 → 245 / 258**、
有限 UV を持つ有効な面が **0 / 8 → 5 / 8** になりました。
トレラント coedge の辺変換は 195件、有限曲面の読み取りは 8件のままです。
保存直線ビュー153件に加え、新しい spline ビューは224件です（直線を含む）。
全33ファイルの有効ソリッド変換は10件（回帰9件、holdout 1件）で、既存の固定ソリッド量を維持しました。

残る13面は、inline coedge 未対応10面、辺ごとの上限も超える pcurve 不整合2面、未対応曲線1面です。
FTC07 全体は引き続き `geometry.pcurve_mismatch` で拒否します。
現在の Model State、Inventor 出力との一致、Windows／macOS、リモート CI、公開配布物の合格を示す結果ではありません。

追加対応の検証には対応する cq-acis ソースをインストールしてください。
上記スクリプトは公開0.3.3でも従来範囲の検査を実行でき、新ビューがある環境では追加の件数・出典・許容差を検査します。
各レポートにパッケージ版と読み込んだモジュールのパスを記録するため、同じ版番号を持つ開発ビルドと公開配布物を区別できます。
