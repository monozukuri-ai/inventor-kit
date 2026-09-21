# トレラント境界と有限 UV

[English](tolerant-trims.md) | 日本語

面を構築する前に、保存された辺・頂点・UVの許容値を検査します。
対応する保存UV fitは、元の軌跡と順序を保ちながら再パラメータ化できます。
元の頂点座標と3D曲線は固定します。頂点許容値を辺・曲線・UVの許容値へ流用せず、
モデル全体の分解能は0.00001 mmを維持します。

未対応または曖昧な許容値の形式は変換を拒否します。個別面の成功だけでは部品全体の対応とせず、
`Document.to_cadquery()`で必要な全ての元の面を保持する有効ソリッドを要求します。
[対応範囲](support.ja.md)と[body選択](body-conversion.ja.md)も参照してください。

## 再現方法

Rustは公開済みの`acis-core=0.3.8`と`acis-py-bridge=0.3.8`を使用します。
Pythonは`cq-acis>=0.3.8,<0.4`と共有モデルAPI 2を必要とします。

```sh
python scripts/validate_components.py
python scripts/validate_topology.py
python scripts/validate_tolerant_trims.py
python scripts/validate_geometry.py
```

入力のハッシュと独立holdoutを維持し、元の許容値と実測誤差を`internal/reports/latest/`へ記録します。
比較方法と限界は[検証ガイド](validation.ja.md)を参照してください。
