# codexSwitch

Codex アプリ（デスクトップ版）の**2つ目のインスタンス**を、Z.ai や OpenRouter などの別プロバイダで起動し、本体の会話をそこへ引き継ぐ CLI ツールです。

Run a second Codex app instance on another model provider (Z.ai, OpenRouter, ...) and hand conversations from the main app over to it.

> **非公式ツールです。** OpenAI とは無関係で、OpenAI による承認・保証はありません。
> Unofficial. Not affiliated with or endorsed by OpenAI.

## できること

```text
codexSwitch                       第2インスタンスを前回と同じプロバイダ・モデルで起動
codexSwitch -zai                  Z.ai で起動
codexSwitch -openrouter           OpenRouter で起動
codexSwitch handoff <ID|タイトル>  本体の会話を第2インスタンスへ引き継ぐ
codexSwitch status                第2インスタンスと設定の状態を表示
```

macOS と Windows で同じコマンドです。

本体の Codex アプリ（ChatGPT アカウントの OpenAI モデル）には一切手を加えず、その横で別プロバイダ用のアプリを並べて使えます。

```text
~/.codex        → 本体アプリ（ChatGPT / OpenAI）   ← 触らない
~/.codex-switch → 第2インスタンス（Z.ai / OpenRouter）
```

## 仕組み

Codex は `CODEX_HOME` ごとに**プロバイダを1つしか持てず**、アプリのモデル選択からプロバイダを切り替えることもできません。そこで、`CODEX_HOME` と Electron のユーザーデータを分けた2つ目のアプリを起動します。ユーザーデータのディレクトリが違えば、アプリの2重起動の制限にかかりません。

会話の引き継ぎには、Codex CLI に含まれる `codex app-server`（アプリ自身も使っている JSON-RPC）を使います。

## 必要なもの

- Codex アプリ
  - macOS: `/Applications/ChatGPT.app`
  - Windows: Microsoft Store 版（MSIX パッケージ `OpenAI.Codex`）
- Rust（ビルドする場合）
- 第2インスタンス用の設定（下記）

## セットアップ

### 1. API キーをファイルに置く

```sh
printf %s '<Z.ai Coding Plan のキー>' > ~/.codex/zai.key
printf %s '<OpenRouter のキー>'        > ~/.codex/openrouter.key
chmod 600 ~/.codex/zai.key ~/.codex/openrouter.key
```

### 2. 第2インスタンスの設定を作る

`~/.codex-switch/config.toml` を作ります。先頭の**管理ブロック**（2つのコメント行で囲んだ部分）は、このツールが書き換えます。

```toml
# >>> codexSwitch managed: active provider >>>
model_provider = "zai"
model = "glm-5.3-flash"
model_catalog_json = "/Users/you/.codex-switch/zai_models.json"
model_reasoning_effort = "high"
# <<< codexSwitch managed: active provider <<<

[model_providers.zai]
name = "Z.ai Coding Plan"
base_url = "https://api.z.ai/api/v1"
wire_api = "responses"
requires_openai_auth = false

[model_providers.zai.auth]
command = "/bin/cat"
args = ["/Users/you/.codex/zai.key"]

[model_providers.openrouter]
name = "OpenRouter"
base_url = "https://openrouter.ai/api/v1"
wire_api = "responses"
requires_openai_auth = false

[model_providers.openrouter.auth]
command = "/bin/cat"
args = ["/Users/you/.codex/openrouter.key"]
```

- `requires_openai_auth = false` で、第2インスタンスは ChatGPT へのサインインなしで動きます
- `auth.args` のパスは**絶対パス**で書いてください（`~` は展開されません）
- **Windows** では設定の置き場所が `%USERPROFILE%\.codex-switch\config.toml`、キーは `%USERPROFILE%\.codex\*.key` です。キーの読み取りは `cmd /c type` を使います。バックスラッシュをそのまま書けるよう、TOML のリテラル文字列（`'...'`）にします

  ```toml
  [model_providers.openrouter.auth]
  command = 'C:\Windows\System32\cmd.exe'
  args = ['/c', 'type', 'C:\Users\you\.codex\openrouter.key']
  ```

- Z.ai は Codex 用のエンドポイント `https://api.z.ai/api/v1` を使います。Claude Code 用や OpenCode 用とは別です

### 3. モデルカタログを置く

`model_catalog_json` で指定したファイルが、アプリのモデル選択の中身になります。

- **Z.ai**：`GET https://api.z.ai/api/v1/models` が Codex 形式のカタログをそのまま返します

  ```sh
  curl -s https://api.z.ai/api/v1/models \
    -H "authorization: Bearer $(cat ~/.codex/zai.key)" > ~/.codex-switch/zai_models.json
  ```

- **OpenRouter**：自分で書きます。1モデルにつき次の形です

  ```json
  {"models": [{
    "slug": "nex-agi/nex-n2.5-pro:free",
    "display_name": "Nex N2.5 Pro (free)",
    "description": "Vision + computer use",
    "priority": 1,
    "visibility": "list",
    "context_window": 262144,
    "max_context_window": 262144,
    "input_modalities": ["text", "image"],
    "default_reasoning_level": "low",
    "supported_reasoning_levels": [
      {"effort": "low", "description": "Light reasoning"},
      {"effort": "medium", "description": "Balanced reasoning"},
      {"effort": "high", "description": "Enhanced reasoning"}],
    "default_reasoning_summary": "none",
    "base_instructions": "",
    "shell_type": "shell_command",
    "apply_patch_tool_type": "freeform",
    "effective_context_window_percent": 95,
    "experimental_supported_tools": [],
    "support_verbosity": false,
    "supported_in_api": true,
    "supports_parallel_tool_calls": true,
    "supports_reasoning_summaries": true,
    "truncation_policy": {"limit": 10000, "mode": "bytes"}
  }]}
  ```

  > **`codex debug models` の出力（OpenAI のモデルの定義）を複製して作らないでください。** OpenAI のモデル用の `"tool_mode": "code_mode_only"` が含まれていて、新しい版の codex はこれに従ってツールを JavaScript 実行用の入れ物（namespace）にまとめて送ります。OpenAI 以外のモデルはこの形を受け付けず、`tools[0].function: missing field 'parameters'` のようなエラーで止まります。上の形は Z.ai が配信している Codex 向けカタログに合わせたものです

## インストール

### macOS

```sh
git clone https://github.com/ttokunaga-ja/codexSwitch.git
cd codexSwitch
./install.sh            # ~/.local/bin/codexSwitch に入ります
```

### Windows

次のどちらかを、PATH の通ったフォルダ（例: `%USERPROFILE%\.local\bin`）に置きます。

- **exe 版**：MSVC 版の Rust（`stable-x86_64-pc-windows-msvc`）で `cargo build --release` した `target\release\codexSwitch.exe`、または GitHub Actions の成果物（`codexSwitch-Windows`）
- **スクリプト版**：`windows\codexSwitch.cmd` と `windows\codexSwitch-launch.ps1` の2つ。起動（`codexSwitch`、`codexSwitch -zai` など）だけを行います。スマート アプリ コントロールで exe が止められる環境向けです

  ```powershell
  Copy-Item windows\codexSwitch.cmd, windows\codexSwitch-launch.ps1 "$env:USERPROFILE\.local\bin"
  ```

exe 版とスクリプト版は、どちらか一方だけを置いてください。

## 使い方

### 状態を見る

```text
$ codexSwitch status
第2インスタンス : 停止中
現在の設定      : zai / glm-5.3-flash
プロバイダ:
  openrouter  nex-agi/nex-n2.5-pro:free      キー ✓  カタログ ✓
  zai         glm-5.3-flash                  キー ✓  カタログ ✓
codex           : /Applications/ChatGPT.app/Contents/Resources/codex (codex-cli 0.155.0-alpha.9.2)
本体のホーム    : ~/.codex
第2のホーム     : ~/.codex-switch
```

### 起動する

```sh
codexSwitch
codexSwitch -zai
codexSwitch -openrouter
codexSwitch -openrouter --model inclusionai/ling-3.0-flash-vl:free
```

| 指定 | 起動する内容 |
| --- | --- |
| なし | 前回と同じプロバイダ・モデル |
| `-<プロバイダ>` | そのプロバイダの既定モデル |
| `--model <slug>` | 指定したモデル |

起動中に実行したときは、次のようになります。

- **前回と同じ内容**：第2インスタンスのウィンドウを表示します。×ボタンで閉じたあとに開き直すときにも使えます
- **違う内容**：何もせずに止まります。プロバイダとモデルは起動時に読まれるため、アプリを終了してから、もう一度実行してください

### 終了する

アプリの画面から終了します。このツールがアプリを終了させることはありません。ウィンドウを閉じるだけでは、どちらの OS でもアプリは動き続けます。

- **macOS**：第2インスタンスのウィンドウを前面にして ⌘Q
- **Windows**：タスクバーの通知領域にある第2インスタンスのアイコンを右クリックし、一番下の「Exit」。本体のアプリのアイコンも並ぶので、メニューに表示される会話で見分けてください

### 会話を引き継ぐ

```sh
codexSwitch handoff ログイン画面
codexSwitch handoff ログイン画面 -openrouter
codexSwitch handoff 0199aaaa-bbbb-7ccc-8ddd-eeeeffff0000 --dry-run
```

タイトルの一部か ID で指定します。タイトルは会話の名前を優先して探し、見つからなければ最初のメッセージまで広げます。複数が該当したときは候補を表示して止まります。

実行前に内容を表示し、確認を求めます。

```text
引き継ぎ元   : ログイン画面のバリデーションを修正
               gpt-6-astra / openai / ~/dev/example-app
               id 0199aaaa-bbbb-7ccc-8ddd-eeeeffff0000
引き継ぎ先   : 第2インスタンス（~/.codex-switch）/ zai / glm-5.3-flash
新しい名前   : ログイン画面のバリデーションを修正（glm-5.3-flash）
コピー       : 会話ファイル 1 件（138.4 MB）
最初のメッセージ（読み取り専用で実行）:
    Z.ai（glm-5.3-flash）へ引き継ぎました。ここまでの状況と、次に着手すべきことを3行以内で整理してください。ファイルの変更やコマンドの実行はしないでください。
送信する量   : 約 151,898 トークン（元の会話の直近の入力量）
第2インスタンス: 停止中 → 完了後に zai で起動します

続行しますか？ [y/N]
```

第2インスタンスが起動中のときは、確認のあと、アプリの画面から終了するよう表示して待ちます。終了を確認すると続きを行います。10分待っても終了しなければ、何も変更せずに中止します。

| オプション | 内容 |
| --- | --- |
| `-<名前>`, `--provider <名前>` | 引き継ぎ先のプロバイダ（既定: 第2インスタンスの現在の設定） |
| `--model <slug>` | 引き継ぎ先のモデル（既定: プロバイダの既定モデル） |
| `--message <文>` | 最初のメッセージ（既定: 引き継ぎの要約） |
| `--name <名前>` | 新しい会話の名前（既定: `元の名前（モデル名）`） |
| `--no-relaunch` | 第2インスタンスの終了を待たず、再起動もしない（プロジェクト割り当ても行わない） |
| `--dry-run` | 内容を表示するだけで何もしない |
| `-y`, `--yes` | 確認を省略する |
| `--timeout <秒>` | 最初のメッセージの完了を待つ時間（既定: 600） |

## 引き継ぎで行うこと

1. 本体（`~/.codex`）から会話を探す（読み取り専用）
2. 会話ファイルを第2インスタンスへコピーする。フォークした会話なら、フォーク元までたどってすべてコピーする。コピー後はハッシュで検証する
3. 確認のうえ、第2インスタンスが起動中なら、アプリの画面から終了されるのを待つ
4. `app-server` の `thread/fork` で、引き継ぎ先のプロバイダ・モデルの会話を作る
5. 名前を付け、最初のメッセージを送る
6. アプリの一覧に表示されることを確認する
7. 会話の作業ディレクトリから、第2インスタンスのプロジェクトを探して割り当てる
8. 第2インスタンスを起動する

途中で失敗しても第2インスタンスは必ず起動し直します。失敗したときは、**元のプロバイダ**で起動します。

### 最初のメッセージについて

- **必要な理由**：メッセージが1件もない会話は、アプリの一覧に表示されません
- **読み取り専用で実行**：誰も見ていない自動実行なので、ファイルの変更やコマンドの実行はできません。文面を変えても同じです
- **コスト**：引き継いだ**会話全体**がモデルに送られます。長い会話ほど、サブスクの利用枠や無料枠を多く使います。送る量の目安は確認画面に表示されます
- **既定の文面**：状況の要約を頼みます。返答を見れば、文脈が正しく渡ったかを確認できます

作業の続きは、アプリの画面で行ってください。引き継いだ会話の権限設定は、元の会話のものを受け継ぎます。作業を再開する前に、画面上の権限設定を確認してください。

## 設定ファイル（任意）

既定値のままで使えます。変えたいときだけ `~/.config/codex-switch/config.toml`（または `$CODEX_SWITCH_CONFIG`）を置きます。

```toml
source_home   = "~/.codex"
sidecar_home  = "~/.codex-switch"
user_data_dir = "~/Library/Application Support/codex-switch/user-data"
app_path      = "/Applications/ChatGPT.app"
# codex_bin   = "codex"   # 既定はアプリ同梱の codex

[providers.openrouter]
label    = "OpenRouter"
model    = "nvidia/nemotron-3-ultra-550b-a55b:free"
catalog  = "~/.codex-switch/model_catalog.json"
effort   = "low"
key_file = "~/.codex/openrouter.key"
```

`[providers.<名前>]` を書くと、同じ名前の既定の定義を置き換えます。新しい名前を書けば、プロバイダを追加できます（第2インスタンスの `config.toml` にも同じ名前の `model_providers` が必要です）。追加したプロバイダも `-<名前>` で指定できます。

Windows のスクリプト版はこの設定ファイルを読まず、既定値（`zai` と `openrouter`）で動きます。

## 注意事項

- **Codex アプリの内部の挙動に依存しています。** 2つ目のインスタンスの起動（`CODEX_ELECTRON_USER_DATA_PATH`）と、プロジェクトの割り当て（`.codex-global-state.json`）は、公開された仕様ではありません。アプリの更新で動かなくなる可能性があります。このリポジトリには、アプリから取り出したコードは含まれていません
- `app-server` は Codex CLI で `[experimental]` と表示される機能です
- プロジェクトの割り当ては、アプリが止まっている間にだけ行います。起動中のアプリはこのファイルを丸ごと書き直すためです。書き換える前に `.codex-global-state.json.codex-switch.bak` へバックアップします
- 第2インスタンスを終了すると、そこで実行中の作業は止まります
- 引き継いだ会話は、第2インスタンスへコピーした**元の会話ファイルを参照**しています。`~/.codex-switch/sessions/` 以下のコピーは消さないでください
- Z.ai などの別プロバイダの会話を、**本体アプリ**に表示することはできません。本体アプリは自分のプロバイダの会話しか並べないためです
- 最初のメッセージは読み取り専用で実行します。macOS ではファイルの書き込みが拒否されること、Windows ではコマンドの起動が拒否されること（codex の Windows 用サンドボックスによる）を実機で確認しています
- API キーは設定ファイルに書かず、キーファイルから読み込みます

## 対応環境

| OS | 状態 |
| --- | --- |
| macOS | 動作確認済み |
| Windows | 対応。部品ごとに実機で確認済み（下記） |

動作確認した環境:

- macOS 27.0 / Codex アプリ 26.915.31945（同梱 codex-cli 0.155.0-alpha.9.2）/ Rust 1.95
- Windows 11 Pro 25H2 / Codex アプリ 26.917.8451.0（同梱 codex-cli 0.155.0-alpha.16.3）

### Windows での仕組みと注意

Windows 版のアプリは MSIX パッケージなので、macOS とは次の点が違います。

- **起動**：パッケージには実行エイリアスがないため、`Invoke-CommandInDesktopPackage` でパッケージの中で `cmd.exe` を動かし、そこで環境変数を設定してから `ChatGPT.exe` を起動します
- **codex の実行ファイル**：パッケージ内の `codex.exe` は外から実行できない（Access is denied）ため、アプリの版ごとに `%LOCALAPPDATA%\codex-switch\` へ複製して使います（初回のみ約300MB）
- **閉じる・終了する**：×ボタンで閉じても、アプリは通知領域に残って動き続けます。終了は通知領域のアイコンのメニューから行います。会話の引き継ぎで第2インスタンスを止める必要があるときも、この方法で終了されるのを待ちます
- **実行する場所**：サインイン中のデスクトップのターミナルから実行してください。SSH などから実行すると、アプリは起動しても画面に表示されません（スクリプト版は、この場合エラーで止まります）
- **スマート アプリ コントロール**：オンの環境では、署名のない `codexSwitch.exe` が Windows によって止められることがあります（終了コード 4551）。判定はファイルごとのため、版によって通ったり止められたりします。止められる場合は、起動にはスクリプト版を使ってください

Windows の実機で確認したこと:

- ビルド・静的解析・ユニットテスト（GitHub Actions の Windows 環境）
- 状態表示、アプリのパッケージ検出、codex.exe の初回複製
- 会話のコピー、フォーク、名前付け、最初のメッセージの送信、失敗時の再起動
- 2つ目のインスタンスの起動（本体のアプリには影響しない）
- スクリプト版の起動：前回と同じ内容での起動、`-zai`・`-openrouter`・`--model` の指定、起動中の再表示と、違う内容を指定したときに止まること、×ボタンで閉じたあとにウィンドウが戻ること
- 読み取り専用の最初のメッセージでコマンドが起動されず、ファイルも作られないこと

スマート アプリ コントロールに止められたため、最終版の `codexSwitch.exe` での通し実行（終了の待機、最初のメッセージの完了からプロジェクト割り当て、再起動まで）は未確認です

## ライセンス

[MIT](LICENSE)
