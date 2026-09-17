# IDWの構造一覧

[English](drawing-inventory.md) · [図面コーパス](drawing-validation.ja.md)

`inventor_core::drawing::inspect(bytes, source_id, limits)` で、文書属性と処理量を制限したRSe構造一覧を取得できます。
**型に基づく暫定フィールド抽出**にも対応します。任意実行の試験シーンで保存された配置と一部図形を解釈できますが、正式なシート選択、適格化済み描画、Autodeskとの比較は未対応です。
[実験的なPython APIとViewer](drawing.ja.md)で保存シート一覧と対応する表示空間を取得できます。単位と実図面の正確性は未確定です。

リポジトリのcheckoutから実行します。

```bash
cargo run --locked -p inventor-core --example inspect_drawing -- fixtures/public/SampleBg.idw
python scripts/validate_drawing_inventory.py
```

exampleは入力ごとにJSONを1行出力します。検証スクリプトはexampleをbuildし、回帰用IDW3件のhashと構造snapshotを照合します。
さらにolefileとzlib/zstandardで、記録したバイト範囲を照合します。
既定の出力先は `internal/reports/latest/drawing_inventory.json` です。
保留検証用図面の内容はこの検証とfuzz seedに使いません。Rust単体テストは合成CFBを使うため実図面は不要です。

## 受理する構造プロファイル

| 境界 | 条件 |
| --- | --- |
| 文書 | 図面root CLSIDで識別され、文書種別の矛盾がない |
| DB | 完全に読めたschema31のRSeDbが一つだけある |
| 所属 | 文書領域のregistry IDとMeta IDが一意に対応し、名前も一致する |
| segment major | 31のみ。major23/24は識別情報と未対応診断を残す |
| segment kind | `DlDocDcSegmentType`、`DlBRxSegmentType`、`DlDirectorySegmentType`、`AppSegmentType`、`FBAttributeSegment`、`DlSheetDcSegmentType`、`DlSheetSmSegmentType`、`DlSheetDlSegmentType` |
| Meta | 版8。DocDC/SheetDCのsection 9は15バイト幅、他の対応kindは19バイト幅。型記述子はDocDCのみ最大4096件、他は256件 |
| B stream | 観測済み18-byteヘッダー `9ec22ba4d411e80160002db3ee29fbb00402` と、後続する単一zstd frame |
| record | 有効なMeta block slot、下位byteの型selector、payload長、対応範囲の拡張trailer、終端marker |

プロファイル名は `idw-rse31-meta8-major31-zstd-framing-v2` です。
kind・majorが一致しても、Meta/Bの構造検査は省略しません。
ヘッダー、codec、table構造、trailerが範囲外なら、そのsegmentを拒否します。
Metaの展開は共通のzlib/zstd処理を使います。
bulkの終端marker以降は未解釈範囲として残し、その内部構造や完全性までは保証しません。

文書segmentのowner候補は `/RSeStorage/M*` に限定します。
Templatesなどの入れ子領域は、名前・IDが似ていても未解釈です。
通常のMetaとはヘッダーが異なるため、文書用の構造で読んで名前やIDを決めません。
これらも処理量に計上して一覧へ残します。孤立したB streamからレコードを推測しません。
文書領域の重複ID、識別不能Meta、名前不一致、複数DBでは、該当する所属を確定しません。

## 出力と根拠

元の属性・サムネイル、source SHA-256、registry、IDの対応、受理したprofile、
Meta block word、完全な型GUID、圧縮情報、record ordinal、診断、未所属stream、未解釈範囲、暫定 `observations` を出力します。
`sheet_count` は常にnull、`drawing_semantics` は `not_decoded` です。
`DLSheet128...` のような名前を、シート数・順序の証拠にはしません。

位置は指定CFB stream内、またはその展開後のバイト列内の半開区間 `[start_offset,end_offset)` です。
**物理ファイル全体のoffsetではありません**。展開後のMeta/Bには、元の圧縮データ範囲も対応付けます。
ordinalとtype indexは0始まりで、無効なMeta blockもslot番号を維持します。
selectorの上位bitは意味を付けずに保持し、先頭部分が同じ型GUIDも完全な値で区別します。

trailerの参照候補は、構造として読めた `raw_name`・`raw_value` とその範囲だけを持ち、
`unresolved_not_followed` とします。payloadから数字・文字・signatureを探して参照を推測しません。
参照の解決・走査を行わないため、繰り返しや循環する値で再帰読込が起きません。
payloadとtrailerの残る意味情報は、未解釈として保持します。

`framed_subset` はregistryの全行が対象の構造検査を通った状態で、Templatesやpayloadの意味解読は含みません。
`partial` は一部が通り、一部が読めなかった状態、`identity_only` は受理したrecordがない状態です。
CFB破損やファイル上限超過はエラー、図面profile未対応は属性と診断を返します。
record tableの途中で失敗したsegmentから、途中までのrecordを受理済みとして返しません。

## 上限と検証範囲

既存の `Limits` を使い、ファイル・stream・属性の処理量を制限します。
図面一覧ではさらに、無視するTemplatesを含む全RSe streamの宣言サイズ、Meta/Bを横断した展開量、
registry object・ID照合・Meta table・record slot・trailer property・参照項目・暫定フィールドのUTF-16単位数と数値数を集計します。
文書Metaは一度読み込んで保持し、一意なMeta/Bの組は一度だけ展開します。
失敗時にも使用した予算を戻しません。展開器のエラーで生成量を測れない場合は、残りの展開予算を使い切った扱いにします。
この集計はRSe一覧処理に対するもので、既存の属性検査にはその上限が別途適用されます。

現在の回帰入力では、`SampleBg.idw` の**全8 segment、計5733 record**の構造を確認できます。
DocDCの4749件とSheetDCの153件も含みます。DocDCの型記述子は303件ありますが、
観測されたselectorの型indexは0〜197です。256以降の記述子も保持し、selectorの幅は広げません。
両DCの後方section 9は15バイト幅で、前方型tableのfooterまで連続して一致します。
他のkindおよび既存IPT/IAM用Metaの構造条件は変更していません。
`_Fishing Rod Assembly.idw`（major23）と `Template_IACS.idw`（major24）は識別情報までです。
これはparserの回帰観測であり、図面の正確性を独立に検証した結果ではありません。

`tests/data/drawing-inventory-baseline.json` で観測と入力hashを結び付けています。
テストは、完全な型ID、無効slot、owner重複・欠落、Templates、元バイト範囲、
trailer破損・切断、未知codec、連結frame、総処理量を対象にします。
追加した `drawing` fuzz targetは、CFB、Meta/Bの組、未圧縮のrecord構造を検査し、通常のfuzz実行にも含めています。
時間を制限したfuzzの成功は、網羅的な安全性の証明ではありません。

## シート・座標・文字の暫定フィールド

`observations` はmajor31の対応条件、segment kind、**完全な型GUID**で選び、
そのrecord型内の構造から値を読みます。本番readerでpayload全体から文字列・数値を検索しません。
全フィールドに出典範囲と元の符号化形式を残し、解釈の状態は `unqualified` とします。
構造が合わない場合は `drawing.fields_unavailable` を返し、そのobservation全体を破棄します。
record境界と未解釈範囲は保持します。

| 解釈候補 | SampleBgの件数 | 読める元の値 |
| --- | ---: | --- |
| 文書のシート一覧 | 1 | 文書ラベルと順序を保った参照値一覧。後続の文書状態は未解釈 |
| シート用segmentへの関係 | 1 | DC/DL/SMの3つのsegment名とラベル |
| シート名 | 1 | 長さ付きUTF-16の `Blatt` |
| シート座標系 | 1 | 表示・定義参照、圧縮変換と、末尾が42・29.7の4倍精度値 |
| シート配置 | 3 | 枠・表題欄・ビューの参照、子参照、圧縮変換 |
| 保存点列 | 19 | 長さ付きf32のXYZ列。元のZも保持 |
| 保存線分・円・円弧 | 73・2・9 | f64の端点、または中心・法線・軸・半径・角度範囲 |
| 属性一覧・真偽値・色 | 149・2・2 | タグなしローカル属性参照、maskと真偽値、RGBAパラメーター |
| レイヤー参照・定義 | 145・50 | Meta参照とファイル内の保存定義、線幅・色・線種 |
| 線の個別指定 | 15 | 線幅、符号付きの破線列、未解釈パラメーター |
| 画像配置 | 2 | 寸法、アフィン変換、形式、埋め込みstream参照 |
| フォント表 | 1 | 69項目のID、名前、寸法・太さ候補 |
| 保存文字 | 55 | UTF-16原文、6倍精度値、生のフラグとstyle index候補 |
| 表示グループ | 76 | 生の子参照値と任意の圧縮変換。DL 74件、SM 2件 |

計607件の観測値は、画面に表示される要素数ではありません。NUL・サロゲートペア・書式文字も保持します。
DocDC内の同型文字は定義の可能性があるため、表示文字として抽出しません。
構造一覧では参照と座標を元の値で保持し、投影・尺度変換やcm単位の断定を行いません。
表示ヘッダーのobject keyとowner参照、Meta section 7・8・10の元bytesと範囲も保存します。
後述の試験解釈は、これらの意味を適格化するものではありません。

`sheet_count=null`、適格化済みの図面意味を示す `drawing_semantics=not_decoded` は維持します。
検証スクリプトは全フィールドを元bytesから別実装で再計算し、f32のbit表現、変換mask、
値のdigestも照合します。これは抽出の検証であり、図面としての正しさの比較値ではありません。
合成入力ではラベル・IDの変更、非ゼロZ、Unicode、切断、未知符号化、非有限値、上限、owner重複を検証します。
正式なDisplayList・描画器へ進むには、変更条件が既知の実図面、別のmajor31入力、
Windows/Autodeskの比較値が引き続き必要です。

## 保存表示情報の試験シーン

```bash
cargo run --locked -p inventor-core --example inspect_drawing_scene -- fixtures/public/SampleBg.idw
```

exampleは `{ "inventory": ..., "sheets": ..., "preview": ..., "images": ... }` を出力します。
Rustでは `drawing::experimental_scene(&inventory, &limits)` を使えます。
`inspect` の構造一覧を入力とし、参照先モデルは開きません。追加処理の上限は `limits.max_records` です。
結果は常に `qualified=false` で、解釈できた場合も `experimental_partial` に留めます。
失敗した座標系は途中の図形を返さず、診断を残します。

観測したmajor31の構造では、SMの表示参照をMeta section 10 → 8 → 7でたどり、
context GUIDの一致、registry segment GUIDの一意性、表示objectのflagsとIDを照合します。
グループの子参照と親参照の相互一致を要求し、参照欠落・重複・循環・重複配置・
不正なアフィン行列・深さ128超過を拒否します。列ベクトルで親行列 × ローカル行列の順に適用します。
branch 0でローカル行列がない場合は、試験解釈として親を引き継ぎます。構造一覧の値は変更しません。

各図形にXYZ、出典範囲、SM配置・親グループの経路、適用行列を残します。
線分・点列は変換済みの点、円・円弧は中心・2本の半径ベクトル・角度範囲で保持します。
文字は位置・方向と任意のフォント候補を持ち、フォント表の行番号ではなく保存IDで照合します。
高さ・太さ・レイアウトは未確定です。フォントが解決できない場合はnullを返します。
未対応の到達ノードと、シートから参照できない既知図形は `omitted` に記録します。
走査範囲外の未解釈レコードは構造一覧に残り、表示上の欠落件数には算入しません。

固定したSampleBg回帰入力では、4つのDL配置と2つのSM画像配置から、
**線分・折れ線91件、円・円弧11件、文字53件、画像2件**を生成します。
部品周囲の矩形は、付属するmask 4・falseの属性で除外します。特定のordinal・寸法・座標では判定しません。
非表示属性は子へ引き継ぎます。シート参照のない文字2件は、ラベルから位置を推定せず省略したままです。

## 表示属性と埋め込み画像の試験対応

`DisplayItem.style` にレイヤーから継承した線幅、個別線幅、正負で区切られた破線列、
取得できたRGBA、出典範囲、未解釈項目 `unresolved` を残します。
属性参照の重複・欠落、レイヤーobject keyの重複、不正な数値では該当座標系を拒否します。
未対応の属性型は明示して保持します。未知のレイヤー線種はnullとし、翻訳済みレイヤー名から破線を作りません。
診断用表示は自動色が未解決なら黒、線幅が未取得なら診断用の既定幅を使います。線幅の単位をmmとは断定しません。

一部のレイヤー参照は古いrevision/context GUIDを指します。
Appのレイヤー保存定義に限り、segment GUIDと完全なobject keyが同一ファイル内で一意なら試験表示へ使い、
`layer_revision_binding_unverified` と表・定義の出典を残します。
シートから表示図形へのcontext一致条件は緩めず、外部ファイルも読みません。
過去リビジョンの値と現在の保存定義が同一であることは未検証です。

文字は生の整列フラグ、変換済みの基線・上向きベクトル、高さ、任意の正の幅係数、フォントフラグを保持します。
診断用描画は2本のベクトルで回転・反転を反映し、複数行もSVGの文字ノードとして作成します。
保存文字をHTMLとして解釈しません。基線フラグの意味、代替フォント、複数行の間隔、複雑な書式は未確定です。
幅フィールドが0なら未解決として残し、幅0の文字にはしません。

exampleは `images` も出力します。
`drawing::read_embedded_images(bytes, &scene, &limits)` は、表示シーンに含まれる画像の数値参照からのみ、
ファイル内の `/RSeStorage/RefdFiles/RefdFile_N` を選びます。
入力SHA-256とシーンの一致、参照の重複、ファイル・stream・総bytes・処理量・画素数の上限を検査します。
外部ファイル名は使用しません。PNGのchunk境界とCRC、baseline JPEGのmarker・SOF・SOS境界を検査しますが、
このRust処理は画素デコーダーではありません。元の画像bytes、形式、寸法、hash、出典を返し、
失敗した画像はbytesなしの診断にします。実画像2件はPillowでの画素デコードとChromiumでの表示も確認しています。
配置は保存寸法・アフィン変換と試験解釈の左上基準を使い、画像bytesは変更しません。

現在のサンプルでは非表示属性による図形1件と未接続文字2件を省略します。
一般的なクリップ・描画順の意味、複雑な文字書式・注記、現在状態、シート順序、単位、他profileへの適用は、
独立した入力とAutodesk比較値での検証が残ります。
調査用HTMLに加え、同じnativeシーンを使う[実験的API・Viewer](drawing.ja.md)を提供します。円弧分割・既定色・代替フォントは表示用の近似です。
保存サムネイルも、現在状態のAutodesk比較値としては扱いません。
