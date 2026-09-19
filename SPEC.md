# wintasks CLI spec

cron 記法のタスク定義 YAML ファイルを Windows Task Scheduler のタスク登録 XML に変換し、タスクの登録・更新・削除を宣言的に管理する Windows 向け CLI。タスク定義 YAML が desired state であり、`apply` 実行後にシステム上の管理下タスクが YAML の定義に一致する。

管理下タスクとは、Description が `managed-by: wintasks;` で始まるタスクである。この CLI は管理下タスクのみを更新・削除する。

コマンドは 2 つである。

- `wintasks render` — YAML を XML に変換し stdout へ出力する。システムを変更しない純粋変換器である
- `wintasks apply` — XML を生成し `schtasks` でシステムへ反映する

共通オプション:

| オプション | 適用コマンド | 既定値 | 意味 |
|---|---|---|---|
| `--path <path>` | render / apply | `wintasks.yaml` | タスク定義 YAML ファイルのパス。ディレクトリは指定できない |
| `--mount <folder>` | apply | `WinTasks` | タスク登録先のフォルダ |
| `--dry-run` | apply | なし | システムを変更せず変更計画を表示する |
| `--prune` | apply | なし | YAML 定義に存在しない管理下タスクを削除する |

引数が不正な場合は usage を表示して非ゼロ終了する。

## YAML スキーマ

1 ファイルに複数のタスク定義を書ける。ファイル全体はタスク定義のリストである。空のファイルや null ドキュメントはエラーとする。空リスト `[]` はタスク 0 件の正常な desired state として受容する。キーは snake_case であり、定義されていないキーが現れたらエラーとする。

| キー | 必須 | 値 |
|---|---|---|
| `name` | 必須 | タスク名。ファイル内で重複してはならない |
| `trigger` | 必須 | トリガー定義 1 個、またはその配列 |
| `action` | 必須 | アクション定義 1 個、またはその配列 |
| `setting` | 任意 | 実行アカウント設定 |

トリガー定義:

| キー | 必須 | 値 |
|---|---|---|
| `type` | 必須 | `cron` / `startup` / `boot` / `once` / `now` のいずれか |
| `value` | 必須 | type ごとの値。下記トリガー表を参照 |

アクション定義:

| キー | 必須 | 値 |
|---|---|---|
| `command` | 必須 | 実行ファイルのパス |
| `args` | 任意 | コマンド引数の文字列 |
| `working_directory` | 任意 | 作業ディレクトリ。未指定のときは `command` の親ディレクトリを使う。親が取り出せないときは設定しない |

setting 定義:

| キー | 必須 | 値 |
|---|---|---|
| `run_as` | 任意 | 真偽値。文字列 `"true"` / `"false"` も真偽値として受容する |
| `logon_type` | 任意 | `interactive_token` / `s4u` のいずれか。未指定は `interactive_token` |

## タスク名

Task Scheduler 上のタスク名は `<mount>\<先頭トリガーの type>\<name>` である。トリガーが配列のとき、先頭要素の type を使う。

## XML 生成

`render` と `apply` は同一の変換を使う。YAML から生成される XML は schtasks の `/Create /XML` に受理される Task Scheduler XML 形式であり、次の内容を持つ。

- ルート要素は `<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">`
- `RegistrationInfo/Description` は `managed-by: wintasks; def-hash: <def-hash>`。`<def-hash>` はトリガー・アクション・setting の定義内容から算出した SHA-256 の 64 桁 16 進数である。同一定義は同一 hash、定義が 1 文字でも異なれば異なる hash になる。def-hash は name と mount を含まない
- `Principals/Principal` は `RunLevel` と `LogonType` を持つ。`RunLevel` は `run_as: true` で `Highest`、それ以外で `LeastPrivilege`。`LogonType` は setting の `logon_type` を反映する（`s4u` → `S4U`、`interactive_token` → `InteractiveToken`）
- `Settings` は次の値を持つ。`StartWhenAvailable=true`、`DisallowStartIfOnBatteries=false`、`StopIfGoingOnBatteries=false`。これらにより、発火時刻を逃したタスクはできるだけ早く開始され、バッテリ駆動でも実行・継続される。他の Settings 要素は Task Scheduler XML の既定値とする
- `Actions` はアクション定義ごとに 1 個の `<Exec>` を YAML の順序どおり並べる。`Command` は `command`、`Arguments` は `args`（未指定なら要素を書かない）、`WorkingDirectory` は `working_directory` の決定結果（決まらないなら要素を書かない）
- `Triggers` はトリガー定義ごとの変換結果を YAML の順序どおり並べる
- 出力は UTF-8 であり、XML 宣言の encoding 属性は出力エンコーディングと一致する
- `StartBoundary` はローカル時刻であり、タイムゾーンオフセットを付けない

### render の出力

複数のタスク定義がある場合、タスクごとに区切り行 `--- <タスク名> ---` と XML 文書を交互に stdout へ出力する。区切り行は常に出力する。

### トリガーの XML 変換

| type | value の形式 | XML trigger 要素 | 変換 |
|---|---|---|---|
| `cron` | cron 式（後述） | `<CalendarTrigger>` の派生 | cron 式を 1 個以上の trigger に分解する（後述） |
| `startup` | `HH:MM` または `HH:MM:SS` | `<LogonTrigger>` | value を `Delay` 要素に ISO 8601 duration（例: `PT1H30M`）で出力する |
| `boot` | `HH:MM` または `HH:MM:SS` | `<BootTrigger>` | value を `Delay` 要素に ISO 8601 duration で出力する |
| `once` | `YYYY-MM-DD` または `YYYY-MM-DD HH:MM[:SS]` | `<TimeTrigger>` | value を `StartBoundary` に出力する。時刻の省略部分は 0 とする |
| `now` | 未使用 | `<RegistrationTrigger>` | 登録完了時に 1 回発火する |

`value` の形式に合わない入力はエラーである。

### cron 式の構文

分・時・日・月・曜日の 5 フィールドを空白区切りで書く。各フィールドは次のいずれかである。

- `*`（全値）
- 単一値
- `n-m`（範囲。月・曜日では `n` と `m` に名前も使える）
- `*/n`、`n-m/n`、`n/n`（ステップ。`n/n` は「n から最大値まで n 刻み」）
- カンマ区切りのリスト。各要素は単一値または範囲（例: `1-3,5`、`MON,WED,FRI`）

フィールドごとの制約:

| フィールド | 値の範囲 | 名前表記 | 備考 |
|---|---|---|---|
| 分 | 0-59 | なし | |
| 時 | 0-23 | なし | |
| 日 | 1-31 | なし | `?` を受容する |
| 月 | 1-12 | `JAN`-`DEC` | 月のステップ（`*/5` など）はエラー |
| 曜日 | 0-6（0=日曜、1=月曜 … 6=土曜） | `SUN`-`SAT` | 名前と数値は混在できる。曜日のステップ（`MON/3` など）はエラー。`?` を受容する |

- 曜日の数値は 0-6 のみとする（7 はエラー）
- `?` は日・曜日フィールドで、フィールド全体がちょうど `?` 1 文字のときだけ受容し、`*` と同等に扱う
- 名前表記は大文字小文字を区別しない
- 上記以外の構文（`L`、`W`、`#`、`@daily` などのマクロ、リストとステップの混在 `1,3/2` など）はエラーである

### cron 式の trigger 分解

Task Scheduler の 1 trigger は連続した等間隔の発火しか表現できないため、cron 式の意味する発火時刻の集合を、1 個以上の trigger に分解して表現する。日・月・曜日のパターンが trigger 種別（`ScheduleByDay` / `ScheduleByWeek` / `ScheduleByMonth` / `ScheduleByMonthDayOfWeek`）を決め、時・分のパターンが trigger 数と Repetition を決める。

trigger 種別の選択:

| 曜日 | 日 | 月 | 生成する trigger |
|---|---|---|---|
| `*` | `*` | `*` | ScheduleByDay（DaysInterval = 日の Increment。`*/n` の日なら n）1 個 |
| `*` | `*` | 非 `*` | ScheduleByMonth（Day は 1-31 の全値、Months は指定月）1 個 |
| `*` | 非 `*` | 任意 | ScheduleByMonth（Day は日の値のリスト、Months は指定月）1 個 |
| 非 `*` | `*` | `*` | ScheduleByWeek（曜日リスト）1 個 |
| 非 `*` | `*` | 非 `*` | ScheduleByMonthDayOfWeek（曜日リスト、指定月）1 個 |
| 非 `*` | 非 `*` | 任意 | ScheduleByMonth（日のリスト）と ScheduleByMonthDayOfWeek（曜日リスト）の 2 個。両 trigger が独立に発火し、OR セマンティクスになる |

- `*` 月のときは月の制約を付けない（全月が対象）
- ScheduleByMonthDayOfWeek は毎週（Weeks 要素を書かない）とする

時・分パターンごとの trigger 分解（`List` はカンマリスト、`Step` は `n/n`・`n-m/n`・`*/n` のステップ値、`Range` は `n-m` の連続範囲、`Start` はフィールドの最小値）:

| 分 | 時 | trigger 数 | StartBoundary の時刻 | Repetition Interval | Repetition Duration |
|---|---|---|---|---|---|
| `*`（Step を含む） | `*` または Range | 1 | `時のStart`:00 | 分の Increment（`*` なら 1 分） | 時の Range 幅（`end - start + 1` 時間） |
| `*` | List または Step | 時の値ごと | `h`:00 | 分の Increment | 1 時間 |
| List または Step（List のときは各値） | `*` または Range | 分の値ごと | `時のStart`:`m` | 1 時間 | 時の Range 幅 |
| Range または Step | List または Step | 時の値ごと | `h`:`分のStart` | 分の Increment | 分の Range 幅（`end - start + 1` 分） |
| List | List または Step | 時 × 分の全組み合わせ | `h`:`m` | なし | なし |

- Repetition は `<Repetition><Interval>` と `<Repetition><Duration>` 要素で出力し、Duration の発火境界より手前の発火を表現する
- 例: `00 09 * * *`（毎日 9 時）→ ScheduleByDay（DaysInterval=1）+ StartBoundary 当日 09:00、Repetition なし、1 trigger
- 例: `0,30 9,21 * * *` → 毎日 9:00 / 9:30 / 21:00 / 21:30 の 4 trigger
- 例: `*/15 9-17 * * *` → ScheduleByDay + StartBoundary 当日 09:00、Interval=15 分、Duration=9 時間、1 trigger

### StartBoundary の決定と過去補正

- 各 trigger の StartBoundary の日付部分は render / apply 実行日とする
- cron トリガーの StartBoundary が実行時点のローカル時刻以下なら、StartBoundary を +1 日する。1 回のみ補正し、補正後も過去ならそのままにする
- 補正は `cron` type の trigger のみに適用し、`startup` / `boot` / `once` / `now` には適用しない

## apply

次の順序で処理する。

1. YAML を読み込みパースする。パースエラー・name 重複・XML 生成エラーのいずれかがあった場合は、システムを変更せずエラー終了する
2. `schtasks /Query /XML` でシステム上の全タスクの XML を取得し、Description が `managed-by: wintasks;` で始まるタスクのみを管理下タスクと判定する。マーカーの照合はフォルダを問わない
3. 各タスク定義を管理下タスクと比較し分類する

| 条件 | 分類 | apply の動作 |
|---|---|---|
| 同名の管理下タスクがない | `create` | `schtasks /Create /XML <xml> /TN <タスク名> /F` で登録する |
| 同名があり def-hash が異なる | `update` | 同上（/F で上書き） |
| 同名があり def-hash が一致する | `no-change` | 何もしない |

生成したタスク名と同名の非管理下タスク（マーカーなし）がシステムに存在する場合は、そのタスクを登録せずエラー報告して処理を続行する（非管理下タスクを上書きしない）。

4. `--prune` 指定時、管理下タスクのうちタスク定義から生成したタスク名の集合に含まれないものを `schtasks /Delete /TN <タスク名> /F` で削除する
5. 実行した処理をタスクごとに報告する

- `--dry-run` では step 2 までを実行し、各タスクの分類（`--prune` 時は削除対象も）を表示して正常終了する。システムは変更しない
- schtasks の呼び出し失敗（権限不足など）は処理を続行し、最後に失敗したタスク名と schtasks のエラーを報告して非ゼロ終了する
- apply は管理者権限へ自動昇格しない

### 終了コード

| 状況 | 終了コード |
|---|---|
| 成功（変更の有無を問わない）、`--dry-run` | 0 |
| YAML パースエラー、XML 生成エラー、schtasks の失敗 | 非ゼロ |

### エラー表示

エラーは stderr に出力する。YAML パースエラーには発生箇所（ファイル名と行番号）、XML 生成エラーにはタスク名と原因、schtasks 失敗にはタスク名と schtasks の標準エラー出力を含める。
