# codexSwitch のスクリプト版（Windows）: Codex アプリの第2インスタンスを起動する
#
#   codexSwitch                          前回と同じプロバイダ・モデルで起動
#   codexSwitch -zai                     Z.ai で起動
#   codexSwitch -openrouter              OpenRouter で起動
#   codexSwitch -openrouter --model <slug>
#
# codexSwitch.exe の起動と同じ動作をする。スマート アプリ コントロールなどで
# 署名のない exe を実行できない Windows 向け。codexSwitch.cmd から呼ばれる。
# 会話の引き継ぎ（handoff）と状態表示（status）は exe が必要。
# 設定ファイル（~/.config/codex-switch/config.toml）は読まず、既定値で動く。
$ErrorActionPreference = 'Stop'

$sidecarHome = Join-Path $env:USERPROFILE '.codex-switch'
$userData = Join-Path $env:LOCALAPPDATA 'codex-switch\user-data'
$config = Join-Path $sidecarHome 'config.toml'
$providers = [ordered]@{
  openrouter = @{ Model = 'nex-agi/nex-n2.5-pro:free'; Catalog = 'model_catalog.json'; Effort = 'low';  Key = 'openrouter.key' }
  zai        = @{ Model = 'glm-5.3-flash';             Catalog = 'zai_models.json';    Effort = 'high'; Key = 'zai.key' }
}
$managedBegin = '# >>> codexSwitch managed: active provider >>>'
$managedEnd = '# <<< codexSwitch managed: active provider <<<'
$quitHowto = 'タスクバーの通知領域にある第2インスタンスのアイコンを右クリックし、' +
  '一番下の「Exit」を選ぶ（×ボタンで閉じても、通知領域で動き続けます）'

function Fail($message) {
  [Console]::Error.WriteLine("エラー: $message")
  exit 1
}

function Step($message) { Write-Output "▶ $message" }

# The app itself; its helper processes carry --type=.
function Running {
  Get-CimInstance Win32_Process -Filter "Name='ChatGPT.exe'" | Where-Object {
    $_.CommandLine -like "*--user-data-dir=$userData*" -and $_.CommandLine -notmatch '--type='
  }
}

# The package has no execution alias, so cmd.exe is run inside it; cmd sets
# the variables and starts the app, which inherits them.
function Start-Sidecar {
  $pkg = Get-AppxPackage OpenAI.Codex | Sort-Object Version -Descending | Select-Object -First 1
  if (-not $pkg) { Fail 'Codex アプリ（パッケージ OpenAI.Codex）が見つかりません' }
  $exe = Join-Path $pkg.InstallLocation 'app\ChatGPT.exe'
  New-Item -ItemType Directory -Force $userData | Out-Null
  $cmdArgs = "/c set ""CODEX_HOME=$sidecarHome"" && set ""CODEX_ELECTRON_USER_DATA_PATH=$userData"" " +
    "&& start """" ""$exe"" ""--user-data-dir=$userData"""
  Invoke-CommandInDesktopPackage -PackageFamilyName $pkg.PackageFamilyName -AppId App `
    -Command 'cmd.exe' -Args $cmdArgs
}

function TomlString($s) { '"' + $s.Replace('\', '\\').Replace('"', '\"') + '"' }

# --- arguments ------------------------------------------------------------
$provider = $null
$model = $null
for ($i = 0; $i -lt $args.Count; $i++) {
  $arg = [string]$args[$i]
  if ($arg -in '--model', '-model') {
    if ($i + 1 -ge $args.Count) { Fail '--model のあとにモデル名を指定してください' }
    $i++
    $model = [string]$args[$i]
  } elseif ($arg -in '-h', '--help', '/?') {
    Get-Content $PSCommandPath -Encoding UTF8 | Select-Object -Skip 2 -First 4 | ForEach-Object { $_.Substring(2) }
    exit 0
  } elseif ($arg -match '^-([A-Za-z0-9][A-Za-z0-9_-]+)$' -and -not $provider) {
    $provider = $Matches[1].ToLower()
  } elseif ($arg -in 'handoff', 'status') {
    Fail "$arg には codexSwitch.exe が必要です。このスクリプト版は起動だけを行います"
  } else {
    Fail "不明な引数です: $arg（使い方: codexSwitch [-zai | -openrouter] [--model <slug>]）"
  }
}

if ((Get-Process -Id $PID).SessionId -eq 0) {
  Fail 'サインイン中のデスクトップのターミナルから実行してください。SSH などから起動すると、アプリは画面に表示されません'
}

# --- settings -------------------------------------------------------------
if (-not (Test-Path $config)) { Fail "第2インスタンスの設定がありません: $config" }
$text = [IO.File]::ReadAllText($config)
$start = $text.IndexOf($managedBegin)
$end = $text.IndexOf($managedEnd)
if ($start -lt 0 -or $end -lt $start) {
  Fail "$config に管理ブロックがありません。次の2行で囲んだブロックを用意してください:`n  $managedBegin`n  $managedEnd"
}
$block = $text.Substring($start, $end - $start)
function Active($key) {
  if ($block -match "(?m)^$key\s*=\s*[""']([^""']*)[""']") { $Matches[1] } else { '' }
}
$activeProvider = Active 'model_provider'
$activeModel = Active 'model'

$name = if ($provider) { $provider } else { $activeProvider }
$p = $providers[$name]
if (-not $p) { Fail "未知のプロバイダです: $name（設定済み: $($providers.Keys -join ', ')）" }
# Without a provider, start exactly what ran last time.
if (-not $model) { $model = if (-not $provider -and $activeModel) { $activeModel } else { $p.Model } }

$keyFile = Join-Path $env:USERPROFILE ".codex\$($p.Key)"
if (-not (Test-Path $keyFile) -or (Get-Item $keyFile).Length -eq 0) {
  Fail "$name の API キーが空です: $keyFile"
}

# --- launch ---------------------------------------------------------------
# A running instance is never stopped here: quitting is left to the app's UI.
if (Running) {
  if ($name -eq $activeProvider -and $model -eq $activeModel) {
    # The app allows one instance per user-data dir: the new process hands
    # over to the running one, which then shows its window.
    Start-Sidecar
    Step "第2インスタンスは $name / $model で起動中です。ウィンドウを表示しました"
    exit 0
  }
  Fail ("第2インスタンスが $activeProvider / $activeModel で起動中です。プロバイダとモデルは起動時に読まれるため、" +
    "アプリを終了してから、もう一度実行してください`n  終了のしかた: $quitHowto")
}

$newBlock = "$managedBegin`n" +
  "model_provider = $(TomlString $name)`n" +
  "model = $(TomlString $model)`n" +
  "model_catalog_json = $(TomlString (Join-Path $sidecarHome $p.Catalog))`n" +
  "model_reasoning_effort = $(TomlString $p.Effort)`n"
$text = $text.Substring(0, $start) + $newBlock + $text.Substring($end)
$tmp = "$config.codex-switch.tmp"
[IO.File]::WriteAllText($tmp, $text, (New-Object Text.UTF8Encoding $false))
Move-Item -Force $tmp $config

Start-Sidecar
$deadline = (Get-Date).AddSeconds(30)
while (-not (Running)) {
  if ((Get-Date) -gt $deadline) { Fail '第2インスタンスの起動を30秒以内に確認できませんでした' }
  Start-Sleep -Milliseconds 500
}
Step "第2インスタンスを $name / $model で起動しました"
