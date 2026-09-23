# codexSwitch

Codex アプリ（デスクトップ版）の**2つ目のインスタンス**を、Z.ai や OpenRouter などの別プロバイダで起動し、本体の会話をそこへ引き継ぐ CLI ツールです。

Run a second Codex app instance on another model provider (Z.ai, OpenRouter, ...) and hand conversations from the main app over to it.

> **非公式ツールです。** OpenAI とは無関係で、OpenAI による承認・保証はありません。
> Unofficial. Not affiliated with or endorsed by OpenAI.

## できること

```text
codex-switch status                     第2インスタンスと設定の状態を表示
codex-switch launch [zai|openrouter]    第2インスタンスを指定プロバイダで起動
codex-switch handoff <ID|タイトル>       本体の会話を第2インスタンスへ引き継ぐ
```

本体の Codex アプリ（ChatGPT アカウントの OpenAI モデル）には一切手を加えず、その横で別プロバイダ用のアプリを並べて使えます。

```text
~/.codex      → 本体アプリ（ChatGPT / OpenAI）   ← 触らない
~/.codex-or   → 第2インスタンス（Z.ai / OpenRouter）
```

## 仕組み

Codex は `CODEX_HOME` ごとに**プロバイダを1つしか持てず**、アプリのモデル選択からプロバイダを切り替えることもできません。そこで、`CODEX_HOME` と Electron のユーザーデータを分けた2つ目のアプリを起動します。ユーザーデータのディレクトリが違えば、アプリの2重起動の制限にかかりません。

会話の引き継ぎには、Codex CLI に含まれる `codex app-server`（アプリ自身も使っている JSON-RPC）を使います。

## 必要なもの

- macOS と Codex アプリ（`/Applications/ChatGPT.app`）
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

`~/.codex-or/config.toml` を作ります。先頭の**管理ブロック**（2つのコメント行で囲んだ部分）は、このツールが書き換えます。

```toml
# >>> codex-or managed: active provider >>>
model_provider = "zai"
model = "glm-5.3-flash"
model_catalog_json = "/Users/you/.codex-or/zai_models.json"
model_reasoning_effort = "high"
# <<< codex-or managed: active provider <<<

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
- Z.ai は Codex 用のエンドポイント `https://api.z.ai/api/v1` を使います。Claude Code 用や OpenCode 用とは別です

### 3. モデルカタログを置く

`model_catalog_json` で指定したファイルが、アプリのモデル選択の中身になります。

- **Z.ai**：`GET https://api.z.ai/api/v1/models` が Codex 形式のカタログをそのまま返します

  ```sh
  curl -s https://api.z.ai/api/v1/models \
    -H "authorization: Bearer $(cat ~/.codex/zai.key)" > ~/.codex-or/zai_models.json
  ```

- **OpenRouter**：`codex debug models` の出力から1件を複製し、`slug`・`display_name`・`context_window`・`input_modalities` などを書き換えて作ります

## インストール

```sh
git clone https://github.com/ttokunaga-ja/codexSwitch.git
cd codexSwitch
./install.sh            # ~/.local/bin/codex-switch に入ります
```

## 使い方

### 状態を見る

```text
$ codex-switch status
第2インスタンス : 停止中
現在の設定      : zai / glm-5.3-flash
プロバイダ:
  openrouter  nex-agi/nex-n2.5-pro:free      キー ✓  カタログ ✓
  zai         glm-5.3-flash                  キー ✓  カタログ ✓
codex           : /Applications/ChatGPT.app/Contents/Resources/codex (codex-cli 0.155.0-alpha.9.2)
本体のホーム    : ~/.codex
第2のホーム     : ~/.codex-or
```

### 起動する

```sh
codex-switch launch zai
codex-switch launch openrouter --model inclusionai/ling-3.0-flash-vl:free
```

プロバイダは起動時に読まれるため、起動中なら確認のうえ終了してから起動し直します。

### 会話を引き継ぐ

```sh
codex-switch handoff ログイン画面
codex-switch handoff 0199aaaa-bbbb-7ccc-8ddd-eeeeffff0000 --dry-run
```

タイトルの一部か ID で指定します。タイトルは会話の名前を優先して探し、見つからなければ最初のメッセージまで広げます。複数が該当したときは候補を表示して止まります。

実行前に内容を表示し、確認を求めます。

```text
引き継ぎ元   : ログイン画面のバリデーションを修正
               gpt-6-astra / openai / ~/dev/example-app
               id 0199aaaa-bbbb-7ccc-8ddd-eeeeffff0000
引き継ぎ先   : 第2インスタンス（~/.codex-or）/ zai / glm-5.3-flash
新しい名前   : ログイン画面のバリデーションを修正（glm-5.3-flash）
コピー       : 会話ファイル 1 件（138.4 MB）
最初のメッセージ（読み取り専用で実行）:
    Z.ai（glm-5.3-flash）へ引き継ぎました。ここまでの状況と、次に着手すべきことを3行以内で整理してください。ファイルの変更やコマンドの実行はしないでください。
送信する量   : 約 151,898 トークン（元の会話の直近の入力量）
第2インスタンス: 停止中 → 完了後に zai で起動します

続行しますか？ [y/N]
```

| オプション | 内容 |
| --- | --- |
| `--provider <名前>` | 引き継ぎ先のプロバイダ（既定: 第2インスタンスの現在の設定） |
| `--model <slug>` | 引き継ぎ先のモデル（既定: プロバイダの既定モデル） |
| `--message <文>` | 最初のメッセージ（既定: 引き継ぎの要約） |
| `--name <名前>` | 新しい会話の名前（既定: `元の名前（モデル名）`） |
| `--no-relaunch` | 第2インスタンスを終了・再起動しない（プロジェクト割り当ても行わない） |
| `--dry-run` | 内容を表示するだけで何もしない |
| `-y`, `--yes` | 確認を省略する |
| `--timeout <秒>` | 最初のメッセージの完了を待つ時間（既定: 600） |

## 引き継ぎで行うこと

1. 本体（`~/.codex`）から会話を探す（読み取り専用）
2. 会話ファイルを第2インスタンスへコピーする。フォークした会話なら、フォーク元までたどってすべてコピーする。コピー後はハッシュで検証する
3. 確認のうえ、起動中の第2インスタンスを終了する
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
sidecar_home  = "~/.codex-or"
user_data_dir = "~/Library/Application Support/Codex OpenRouter/user-data"
app_path      = "/Applications/ChatGPT.app"
# codex_bin   = "codex"   # 既定はアプリ同梱の codex

[providers.openrouter]
label    = "OpenRouter"
model    = "nvidia/nemotron-3-ultra-550b-a55b:free"
catalog  = "~/.codex-or/model_catalog.json"
effort   = "low"
key_file = "~/.codex/openrouter.key"
```

`[providers.<名前>]` を書くと、同じ名前の既定の定義を置き換えます。新しい名前を書けば、プロバイダを追加できます（第2インスタンスの `config.toml` にも同じ名前の `model_providers` が必要です）。

## 注意事項

- **Codex アプリの内部の挙動に依存しています。** 2つ目のインスタンスの起動（`CODEX_ELECTRON_USER_DATA_PATH`）と、プロジェクトの割り当て（`.codex-global-state.json`）は、公開された仕様ではありません。アプリの更新で動かなくなる可能性があります。このリポジトリには、アプリから取り出したコードは含まれていません
- `app-server` は Codex CLI で `[experimental]` と表示される機能です
- プロジェクトの割り当ては、アプリが止まっている間にだけ行います。起動中のアプリはこのファイルを丸ごと書き直すためです。書き換える前に `.codex-global-state.json.codex-switch.bak` へバックアップします
- 第2インスタンスを終了すると、そこで実行中の作業は止まります
- 引き継いだ会話は、第2インスタンスへコピーした**元の会話ファイルを参照**しています。`~/.codex-or/sessions/` 以下のコピーは消さないでください
- Z.ai などの別プロバイダの会話を、**本体アプリ**に表示することはできません。本体アプリは自分のプロバイダの会話しか並べないためです
- API キーは設定ファイルに書かず、キーファイルから読み込みます

## 対応環境

| OS | 状態 |
| --- | --- |
| macOS | 動作確認済み |
| Windows | 未対応。会話のコピーや引き継ぎの処理自体は共通ですが、2つ目のインスタンスの起動を確認できていません（Windows 版のアプリは MSIX で配布されています） |

動作確認した環境: macOS 27.0 / Codex アプリ 26.915.31945（同梱 codex-cli 0.155.0-alpha.9.2）/ Rust 1.95

## ライセンス

[MIT](LICENSE)
