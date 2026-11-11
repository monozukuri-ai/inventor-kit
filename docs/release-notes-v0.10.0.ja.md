# inventor-kit v0.10.0

[English](release-notes-v0.10.0.md) | 日本語

## 形状変換は任意の extra に

inventor-kit は `cq-acis>=0.4.2,<0.5` に依存します。cq-acis は既定では CadQuery を
入れなくなったので、形状ライブラリを一つ選んでインストールします。

```sh
python -m pip install 'inventor-kit[cadquery]'  # CadQuery の形状、STEP 出力、IAM、writer
python -m pip install 'inventor-kit[occt]'      # CadQuery なしで OCCT の形状だけ
```

**破壊的変更:** `pip install inventor-kit` だけでは解析と共通モデルだけが入ります。
`to_cadquery()`、STEP 出力、アセンブリ、writer を使う場合は `inventor-kit[cadquery]` が必要です。
`inventor-kit[viewer]` はこれを含みます。二つの extra は同じ `OCP` モジュールの別ビルドを
入れるので、どちらか一方を選んでください。

## OCCT の経路

- `doc.convert_bodies(backend="auto" | "cadquery" | "occt")`。`"occt"` では `body.shape` が
  OCCT の `TopoDS_Shape` になり、CadQuery を import しません。status・診断・形状量は
  どちらでも同じで、使われた方は `result.backend` に入ります。
- `result.to_occt(body_ids=..., allow_partial=...)` と `doc.to_occt()` は `TopoDS_Shape` を返します。
  OCCT の結果に対する `to_cadquery()` と `export_step()` は `ValueError` になります。

## 保存された B-spline 近似

手続き的な交線と、ブレンド・ロフト・スイープ・回転のスプライン面を、Inventor が一緒に保存した
B-spline 近似から変換します（cq-acis 0.4.1 と 0.4.2）。`geometry.curve_unsupported` や
`geometry.surface_unsupported` で失敗していた body が変換できるようになります。

その body は保存された fit 許容差の範囲の**派生形状**です。`body.warnings` に code
`geometry.approximated_from_stored_spline` が入り、変換レポート・CLI・viewer に警告が出ます。
`body.approximations` に近似の一覧があります。このライブラリが独自に近似を計算することは
引き続きありません。ねじ山のらせん面のように保存された近似のない手続き的形状は未対応のままです。

変換レポートのスキーマ v1 は、body ごとに任意の `warnings` 配列を持つようになりました。

## 候補の segment major

PmBRep segment major 22（SAB save version 22300）を**候補**のプロファイルとして読みます。
観測済みで検証済みの major と同じ枠組みですが、固定した回帰用 fixture はありません。
その表は `profile_status == "candidate_stored_table"` と警告 `candidate.profile_candidate` を
返します。検証済みの major（19 / 25 / 26 / 28 / 30 / 31）は `supported_stored_table` のままで、
それ以外の major は未対応です。`capabilities()` に `candidate_segment_majors` が入ります。

## 必要環境

Python 3.11 以降。Rust のソースは `acis-core=0.4.2` と `acis-py-bridge=0.4.2` を使います。
