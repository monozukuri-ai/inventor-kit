# 公開検証サマリー

[validation-summary.json](validation-summary.json) は確認済みの集計だけを公開します。
実行種別は `environment`、日付は `recorded_on`、対象範囲は `scope` で区別します。
段階別件数を合わせて「Inventor 全体に対応」と解釈しないでください。

生成元の詳細報告は `internal/reports/` に保存します。
`scripts/summarize_validation.py` が明示的に選択した件数と状態だけを出力し、
パス、詳細診断、機体・ホスト情報、生の属性やモデルデータをコピーしません。
CI の公開 artifact にも同じサマリーだけを使います。

比較方法と限界は [検証ガイド](../docs/validation.md)を参照してください。
