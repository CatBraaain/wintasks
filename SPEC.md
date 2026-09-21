# wintasks CLI spec

cron 記法のタスク定義 YAML ファイルを Windows Task Scheduler のタスク登録 XML に変換し、`--mount` で指定した mount folder 配下のタスクを登録・更新・削除する Windows 向け CLI。YAML が desired state を定め、コマンド実行後に指定した mount folder 配下のタスクが YAML の定義に一致する。

タスクの管理対象は Description ではなく mount folder で決まる。指定した mount folder 配下のタスクは、Description の内容にかかわらずこの CLI が管理する。mount folder の外にあるタスクは変更しない。

コマンドの既定動作はサブコマンドなしの `wintasks` であり、`--mount FOLDER` で指定した mount folder と YAML を同期する。`--mount` を省略したときは `wintasks` を mount folder とする。`wintasks --mount FOLDER --dry-run` はシステムを変更せず差分を表示する。`--path FILE` は読み込む定義ファイルを指定し、省略時は `wintasks.yaml` を使う。`--help` と `-h` は同じヘルプ表示として扱い、他のオプションと同時に指定した場合もヘルプを優先する。

オプション:

| オプション | 既定値 | 意味 |
|---|---|---|
| `--mount FOLDER` | `wintasks` | 同期対象とする Task Scheduler の mount folder |
| `--dry-run` | なし | システムを変更せず変更計画と差分を表示する |
| `--path FILE` | `wintasks.yaml` | FILE の YAML を定義ファイルとして読み込む |
| `-h`, `--help` | なし | ヘルプを stdout に表示して終了する。`--mount` なしでも実行でき、定義ファイルを読み込まず、システムを変更しない |

`wintasks --help` または `wintasks -h` は終了コード 0 で、次の文字列を stdout に出力する。出力の各行と末尾の改行を含めて正本とする。

```text
wintasks - synchronize Windows Task Scheduler tasks from wintasks.yaml

Usage: wintasks [OPTIONS]

Options:
  --mount FOLDER  Synchronize tasks under this Task Scheduler folder (default: wintasks)
  --dry-run       Show planned changes without modifying the system
  --path FILE     Read task definitions from FILE (default: wintasks.yaml)
  -h, --help      Show this help message
```

`--help` または `-h` は `--mount FOLDER`、`--dry-run` または `--path FILE` と同時に指定でき、ヘルプだけを出力する。認識できない引数を含む場合は、ヘルプオプションがあっても usage エラーとなる。`--mount` の直後にオプション形式のトークンがある場合は、フォルダ値の欠落として usage エラーとなる。`--mount=FOLDER` と `--path=FILE` 形式は受け付けず、それぞれ `--mount FOLDER` と `--path FILE` 形式を使う。

定義ファイルは `--path FILE` で指定し、省略時は `wintasks.yaml` を使う。FILE が相対パスのときは、コマンド実行時のカレントディレクトリから解決する。読み込みまたはパースに失敗したときは、指定した FILE をエラーに表示する。引数が不正な場合は usage を表示して非ゼロ終了する。

## YAML スキーマ

ファイル全体はタスク定義の配列である。空の配列はタスク 0 件の正常な desired state として受容する。空のファイルや null ドキュメントはエラーとする。各タスク定義のキーは snake_case であり、定義されていないキーが現れたらエラーとする。

`--mount FOLDER` は Task Scheduler の root ではないフォルダパスである。1 個以上のフォルダ名を `\` で区切った相対パスとし、空文字、`\`、先頭または末尾が `\` の値はエラーとする。

| キー | 必須 | 値 |
|---|---|---|
| `name` | 必須 | タスク定義名。ファイル内で重複してはならない |
| `trigger` | 必須 | トリガー定義 1 個、またはその配列 |
| `action` | 必須 | アクション定義 1 個、またはその配列 |
| `setting` | 任意 | 実行アカウント設定 |

トリガー定義:

| キー | 必須 | 値 |
|---|---|---|
| `type` | 必須 | `cron` / `startup` / `boot` / `once` / `now` のいずれか |
| `value` | 必須 | type ごとの値。下記トリガー表を参照。`now` では必須だが内容は使用しない（任意の文字列を書ける） |

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

## タスクパス

Task Scheduler 上のタスクの完全パスは `\<mount>\<先頭トリガーの type>\<name>` である。トリガーが配列のとき、先頭要素の type を使う。`--mount WinTasks`、`name: backup`、先頭トリガーが `cron` のタスクパスは `\WinTasks\cron\backup` である。

タスク定義の `name` はタスクパスの末尾要素である。同期対象は mount folder 自身と、そのすべての子フォルダにあるタスクである。desired state にない同期対象タスクは削除する。

## XML 生成

既定動作は YAML から XML を生成する。生成される XML は schtasks の `/Create /XML` に受理される Task Scheduler XML 形式であり、次の書式と内容を持つ。

- XML 宣言は `<?xml version="1.0" encoding="UTF-8"?>`。出力は UTF-8 である
- インデントは 2 スペース。子要素を持たない要素は `<X/>` 形式で書く
- ルート要素は `<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">`。ルートの直下要素の順序は RegistrationInfo → Triggers → Principals → Settings → Actions とする
- `RegistrationInfo` に管理対象を示す Description やハッシュを出力しない。タスクの管理対象判定と変更判定は mount folder と XML の内容で行う
- `Principals/Principal` は `RunLevel` と `LogonType` を持つ。`RunLevel` は `run_as: true` で `Highest`、それ以外で `LeastPrivilege`。`LogonType` は setting の `logon_type` を反映する（`s4u` → `S4U`、`interactive_token` → `InteractiveToken`）
- `Settings` は次の値を持つ。`StartWhenAvailable=true`、`DisallowStartIfOnBatteries=false`、`StopIfGoingOnBatteries=false`。これらにより、発火時刻を逃したタスクはできるだけ早く開始され、バッテリ駆動でも実行・継続される。他の Settings 要素は Task Scheduler XML の既定値とする
- `Actions` はアクション定義ごとに 1 個の `<Exec>` を YAML の順序どおり並べる。`Command` は `command`、`Arguments` は `args`（未指定なら要素を書かない）、`WorkingDirectory` は `working_directory` の決定結果（決まらないなら要素を書かない）
- `Triggers` はトリガー定義ごとの変換結果を YAML の順序どおり並べる
- `StartBoundary` はローカル時刻であり、タイムゾーンオフセットを付けない

次の YAML 定義を、実行日が 2026-09-20 で実行時刻が 2026-09-20T09:00:00 より前である場合に変換した出力は:

```yaml
- name: backup
  trigger:
    type: cron
    value: "0 9 * * MON"
  action:
    command: C:\Tools\backup.exe
    args: --full
    working_directory: C:\Backup
```

```xml
<?xml version="1.0" encoding="UTF-8"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo/>
  <Triggers>
    <CalendarTrigger>
      <StartBoundary>2026-09-20T09:00:00</StartBoundary>
      <ScheduleByWeek>
        <DaysOfWeek>
          <Monday/>
        </DaysOfWeek>
      </ScheduleByWeek>
    </CalendarTrigger>
  </Triggers>
  <Principals>
    <Principal>
      <RunLevel>LeastPrivilege</RunLevel>
      <LogonType>InteractiveToken</LogonType>
    </Principal>
  </Principals>
  <Settings>
    <StartWhenAvailable>true</StartWhenAvailable>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
  </Settings>
  <Actions>
    <Exec>
      <Command>C:\Tools\backup.exe</Command>
      <Arguments>--full</Arguments>
      <WorkingDirectory>C:\Backup</WorkingDirectory>
    </Exec>
  </Actions>
</Task>
```

### トリガーの XML 変換

| type | value の形式 | XML trigger 要素 | 変換 |
|---|---|---|---|
| `cron` | cron 式（後述） | `<CalendarTrigger>` の派生 | cron 式を 1 個以上の trigger に分解する（後述） |
| `startup` | `HH:MM` または `HH:MM:SS` | `<LogonTrigger>` | value を `Delay` 要素に ISO 8601 duration（例: `PT1H30M`）で出力する |
| `boot` | `HH:MM` または `HH:MM:SS` | `<BootTrigger>` | value を `Delay` 要素に ISO 8601 duration で出力する |
| `once` | `YYYY-MM-DD` または `YYYY-MM-DD HH:MM[:SS]` | `<TimeTrigger>` | value を `StartBoundary` に出力する。時刻の省略部分は 0 とする |
| `now` | 未使用 | `<RegistrationTrigger>` | 登録完了時に 1 回発火する |

`value` の形式に合わない入力はエラーである。

`Delay` 要素の ISO 8601 duration は、値 0 の単位を省略する（`00:30` → `PT30M`）。全単位が 0 なら `PT0S`。`HH` は 2 桁の数字で、23 を超える値も受容する（`25:00` → `PT25H`）。

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
- フィールド全体をカバーする範囲は `*` と同等に扱う（分の `0-59`、時の `0-23`、日の `1-31`、月の `1-12`、曜日の `0-6`）。ステップ付きも同様で、`0-59/15` は `*/15` と同等
- 開始と終了が等しい範囲 `n-n` は単一値 `n` と同等に扱う
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

- `*` 月のときは Months に全 12 月（`JAN`-`DEC`）を出力する（Task Scheduler XML スキーマ上 Months は必須のため）
- ScheduleByWeek と ScheduleByMonthDayOfWeek は毎週とし、Weeks 要素を書かない
- リスト（カンマ列挙）は昇順に並べ替えて重複を除去する（`3,1,3` → `1,3`）。範囲は全値に展開する（`5-10` → 5、6、7、8、9、10）

時・分パターンごとの trigger 分解。`List` はカンマリスト、`Step` は `n/n`・`n-m/n`・`*/n` のステップ値、`Range` は `n-m` の連続範囲とする。`Start` はフィールドの開始値で、`*` なら最小値、Range なら範囲の開始値、List なら最小要素。`Step` の「値」はステップ展開した各発火値（`10-50/20` なら 10、30、50）:

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

- 各 trigger の StartBoundary の日付部分は `wintasks` 実行日とする
- cron トリガーの StartBoundary が実行時点のローカル時刻以下なら、StartBoundary を +1 日する。1 回のみ補正し、補正後も過去ならそのままにする
- 補正は `cron` type の trigger のみに適用し、`startup` / `boot` / `once` / `now` には適用しない

## 変換テストケース

この節の表は、変換単位のテストケースである。テストコードがこの節を読み、各行の入力定義から XML を生成し、期待 XML との一致を検証する。比較の前には、生成結果と期待 XML から、タグの間にある空白だけのテキストを取り除く。期待 XML は生成結果に含まれていればよく、インデントは比較に含まれない。表のセルには `|` を書いてはならない。

トリガー定義の行は、テスト側が次の最小タスクでラップする:

```yaml
- name: sample
  trigger: { type: <type>, value: "<value>" }
  action: { command: cmd.exe }
```

実行時刻のセルは cron の行では必須であり、それ以外の行では無視される。

| type | value | 実行時刻 | 期待 XML |
|---|---|---|---|
| `startup` | `01:30` | | `<LogonTrigger><Delay>PT1H30M</Delay></LogonTrigger>` |
| `boot` | `00:30` | | `<BootTrigger><Delay>PT30M</Delay></BootTrigger>` |
| `once` | `2030-06-01` | | `<TimeTrigger><StartBoundary>2030-06-01T00:00:00</StartBoundary></TimeTrigger>` |
| `now` | `x` | | `<RegistrationTrigger/>` |
| `cron` | `0 11 * * *` | 2026-01-15 10:00 | `<CalendarTrigger><StartBoundary>2026-01-15T11:00:00</StartBoundary><ScheduleByDay><DaysInterval>1</DaysInterval></ScheduleByDay></CalendarTrigger>` |
| `cron` | `0 9 * * *` | 2026-01-15 10:00 | `<CalendarTrigger><StartBoundary>2026-01-16T09:00:00</StartBoundary><ScheduleByDay><DaysInterval>1</DaysInterval></ScheduleByDay></CalendarTrigger>` |
| `cron` | `*/15 9-17 * * *` | 2026-01-15 10:00 | `<CalendarTrigger><StartBoundary>2026-01-16T09:00:00</StartBoundary><Repetition><Interval>PT15M</Interval><Duration>PT9H</Duration></Repetition><ScheduleByDay><DaysInterval>1</DaysInterval></ScheduleByDay></CalendarTrigger>` |
| `cron` | `0,30 9,21 * * *` | 2026-01-15 10:00 | `<CalendarTrigger><StartBoundary>2026-01-16T09:00:00</StartBoundary><ScheduleByDay><DaysInterval>1</DaysInterval></ScheduleByDay></CalendarTrigger><CalendarTrigger><StartBoundary>2026-01-16T09:30:00</StartBoundary><ScheduleByDay><DaysInterval>1</DaysInterval></ScheduleByDay></CalendarTrigger><CalendarTrigger><StartBoundary>2026-01-15T21:00:00</StartBoundary><ScheduleByDay><DaysInterval>1</DaysInterval></ScheduleByDay></CalendarTrigger><CalendarTrigger><StartBoundary>2026-01-15T21:30:00</StartBoundary><ScheduleByDay><DaysInterval>1</DaysInterval></ScheduleByDay></CalendarTrigger>` |

アクション定義の行は、トリガーを `{ type: now, value: "x" }` として同じ YAML のルート配列でラップする。空のセルのキーは書かない。

| command | args | working_directory | 期待 XML |
|---|---|---|---|
| `cmd.exe` | | | `<Exec><Command>cmd.exe</Command></Exec>` |
| `C:\Tools\a.exe` | `--one` | | `<Exec><Command>C:\Tools\a.exe</Command><Arguments>--one</Arguments><WorkingDirectory>C:\Tools</WorkingDirectory></Exec>` |
| `C:\Tools\a.exe` | | `C:\Data` | `<Exec><Command>C:\Tools\a.exe</Command><WorkingDirectory>C:\Data</WorkingDirectory></Exec>` |

## 同期

`wintasks` は次の順序で YAML と mount folder を同期する。

1. `--path FILE` で指定されたファイル（省略時は `wintasks.yaml`）を読み込みパースし、`--mount FOLDER` の指定値（省略時は `wintasks`）の形式を検証する。パースエラー、mount の形式エラー、name 重複、XML 生成エラーのいずれかがあった場合は、システムを変更せずエラー終了する。
2. `schtasks /Query /XML` でシステム上の全タスクの XML を取得する。`/Query` の出力は BOM 付き UTF-16 または UTF-8 で返るため、BOM で判別してデコードする。BOM がなければ UTF-8 として読む。query 出力の 1 文書をパースできなかったときは、その文書以降を無視し、それまでに読めたタスクだけで処理を続行する。
3. タスクパスが mount folder 自身またはその子フォルダにあるタスクだけを同期対象とする。mount folder の外にあるタスクは分類・変更しない。
4. desired state の各タスクと同期対象の既存タスクをタスクパスで比較し、次のように分類する。

| 条件 | 分類 | 同期時の動作 |
|---|---|---|
| desired state と同一タスクパスの既存タスクがない | `create` | `schtasks /Create /XML <xml> /TN <タスクパス> /F` で登録する |
| 同一タスクパスがあり、正規化した XML が一致する | `no-change` | 何もしない |
| 同一タスクパスがあり、正規化した XML が一致しない | `update` | `schtasks /Create /XML <xml> /TN <タスクパス> /F` で上書きする |
| mount folder 配下にあり、desired state に同一タスクパスがない | `delete` | `schtasks /Delete /TN <タスクパス> /F` で削除する |

mount folder 配下の既存タスクは Description の内容にかかわらず同期対象であり、同一タスクパスのタスクを登録・更新できる。mount folder 外の同名タスクは変更しない。

6. `--dry-run` 指定時は分類までを実行し、システムを変更せず各分類を表示する。`no-change` だけで `delete` がない場合は `No changes.` と 1 行だけ表示する。それ以外では分類を表示し、`update` と `delete` の分類行には正規化した XML の unified diff を続ける（「### --dry-run の diff 表示」）。
7. `--dry-run` でないときは、desired state の定義順に `create`、`update`、`no-change` を処理し、その後 `delete` をタスクパスの昇順で処理する。処理の完了ごとに分類を stdout へ出力する。

schtasks の create / delete の呼び出し失敗（権限不足など）は処理を続行し、最後に失敗したタスクパスと schtasks のエラーを報告して非ゼロ終了する。`/Query` の失敗は分類ができないため、即時にエラー終了する。wintasks は管理者権限へ自動昇格しない。

### --dry-run の diff 表示

`--dry-run` では、`update` の分類行の下に同期対象の既存タスクと生成 XML の unified diff を出力し、`delete` の分類行の下に同期対象の既存タスクと空文書の unified diff を出力する。diff は各分類行の直後から始まり、stdout に出力する。`create` と `no-change` には diff を出力しない。

比較前に XML を正規化する。正規化は XML 宣言を UTF-8 に統一し、空白だけのテキストを除去し、属性名を辞書順に並べ、要素・属性を同じインデントと改行でシリアライズする。XML の要素順と、除外対象以外の要素内容を保持する。`RegistrationInfo/Description` と `CalendarTrigger/StartBoundary` は比較から除外する。schtasks が返すそれ以外の要素は比較対象に含める。

`update` の diff は既存 XML を `--- current`、生成 XML を `+++ desired` とする unified diff である。`delete` の diff は既存 XML を `--- current`、空文書を `+++ desired` とする。

差分の出力例は次のとおりである。

```text
update \WinTasks\cron\backup
--- current
+++ desired
@@
-    <Arguments>--full</Arguments>
+    <Arguments>--full --verify</Arguments>
```

### 終了コード

| 状況 | 終了コード |
|---|---|
| 成功（変更の有無を問わない） | 0 |
| usage エラー（不明な引数・オプションの不正） | 2 |
| ファイル読み取り失敗、YAML パースエラー、mount 形式エラー、XML 生成エラー、`/Query` 失敗、schtasks の失敗 | 1 |

### エラー表示

エラーは stderr に出力する。

即時に終了するエラーは `wintasks: <メッセージ>` の形式である。

- ファイル読み取り失敗: `wintasks: <path>: <入出力エラーの内容>`
- YAML パースエラー: 発生位置が分かるときは `wintasks: <file>:<line>:<col>: <原因>`、分からないときは `wintasks: <file>: <原因>`。name 重複は `wintasks: <file>: duplicate task name `<name>``、空または null ドキュメントは `wintasks: <file>: no task definitions found (file is empty or null)`。mount 形式エラーは `wintasks: <file>: mount must be a non-root folder path`
- XML 生成エラー: `wintasks: <path>: XML generation failed for task `<name>`: <原因>`
- `/Query` の失敗: `wintasks: schtasks /Query /XML failed: <schtasks の標準エラー出力>`

usage エラーは `wintasks: <メッセージ>` に続けて usage を stderr へ出力する。

処理を続行したエラーは、すべての処理の後に 1 行ずつ出力する。

- schtasks の失敗: `error: schtasks /Create /TN <タスクパス> failed: <schtasks の標準エラー出力>` または `error: schtasks /Delete /TN <タスクパス> failed: <schtasks の標準エラー出力>`

処理の報告は stdout へ出力する。通常は 1 行が `<分類> <タスクパス>` の形式で、分類は `create` / `update` / `no-change` / `delete` である。`--dry-run` で変更がない場合は `No changes.` と 1 行だけ出力する。それ以外では分類を YAML の定義順に、その後 `delete` をタスクパスの昇順で出力する。実行時は各処理の完了ごとに出力する。
