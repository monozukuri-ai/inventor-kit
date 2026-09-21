# 利用・開発資料

[English](README.md) | 日本語

- [ライセンス](license.ja.md): 非商用許諾、商用プラン、旧 MIT 部分。
- [API ガイド](api.ja.md): 読み込み、属性、保存候補、アセンブリ、上限。
- [body選択](body-conversion.ja.md): 個別結果、部分STEP、出典ID。
- [CLI・バッチ](cli.ja.md): IPT/IAM処理、JSON、終了code、timeout。
- [ローカル viewer](viewer.ja.md): IPT の保存形状と IAM の保存配置、プレビュー、属性、読み込み診断。
- [実験的なIDW表示](drawing.ja.md): 保存図面のシート、Python API、Viewerの使い方と制限。
- [対応範囲](support.ja.md): 確認済みプロファイルと未対応・未検証の境界。
- [検証](validation.ja.md): 比較方法、保留検証と結果の読み方。
- [開発](development.ja.md): 公開ソースだけを使うビルドと再現手順。
- [リリース](releasing.ja.md): 配布 CI、依存契約、公開条件。
- [形式資料](format-reference.ja.md): 参照した資料と出典。

公開ガイドは英語を標準とし、同じディレクトリの `.ja.md` に日本語版を用意します。
各ページ先頭のリンクで言語を切り替えられます。仕様やコマンドを変更したときは、
両言語を同じ変更で更新してください。

`docs/`には使い方、API契約、対応範囲、貢献者が再現できる手順を置きます。
実装計画・形式調査・資料収集手順は`internal/docs/`へ、検証サマリー・作業記録・比較原値・
ホスト情報はすべて`internal/reports/`へ置いてください。
公開ガイドは`internal/`のファイルに依存させません。
公開契約は Python API と [capabilities.json](../python/inventor_kit/capabilities.json) を基準にします。
