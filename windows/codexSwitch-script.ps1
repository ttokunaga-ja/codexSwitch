# codexSwitch のスクリプト版（Windows）: Codex アプリの第2インスタンスを準備・起動する
#
#   codexSwitch init                     最初の準備（設定とモデル一覧を作り、API キーの置き場所を案内する）
#   codexSwitch                          前回と同じ内容で起動
#   codexSwitch -zai                     Z.ai で起動
#   codexSwitch -openrouter              OpenRouter で起動
#   codexSwitch -openrouter --model <slug>
#
# codexSwitch.exe の init と起動と同じ動作をする。スマート アプリ コントロールなどで
# 署名のない exe を実行できない Windows 向け。codexSwitch.cmd から呼ばれる。
# 会話の引き継ぎ（handoff）と状態表示（status）は exe が必要。
# 設定ファイル（~/.config/codex-switch/config.toml）は読まず、既定値で動く。
$ErrorActionPreference = 'Stop'

$sidecarHome = Join-Path $env:USERPROFILE '.codex-switch'
$userData = Join-Path $env:LOCALAPPDATA 'codex-switch\user-data'
$config = Join-Path $sidecarHome 'config.toml'
$keyDir = Join-Path $env:USERPROFILE '.codex'
$providers = [ordered]@{
  openrouter = @{ Label = 'OpenRouter'; Model = 'nex-agi/nex-n2.5-pro:free'; Catalog = 'model_catalog.json'; Effort = 'low'
                  Key = 'openrouter.key'; BaseUrl = 'https://openrouter.ai/api/v1'; CatalogUrl = $null }
  zai        = @{ Label = 'Z.ai Coding Plan'; Model = 'glm-5.3-flash'; Catalog = 'zai_models.json'; Effort = 'high'
                  Key = 'zai.key'; BaseUrl = 'https://api.z.ai/api/v1'; CatalogUrl = 'https://api.z.ai/api/v1/models' }
}
$managedBegin = '# >>> codexSwitch managed: active provider >>>'
$managedEnd = '# <<< codexSwitch managed: active provider <<<'
$notOurs = "$config は codexSwitch が作った設定ではありません（管理ブロックがありません）。" +
  '別の名前に変えるか消してから、codexSwitch init を実行してください'
$quitHowto = 'タスクバーの通知領域にある第2インスタンスのアイコンを右クリックし、' +
  '一番下の「Exit」を選ぶ（×ボタンで閉じても、通知領域で動き続けます）'

# OpenRouter models checked to run Codex's tool calls, in the minimal form.
# Same as catalogs/openrouter.json (a test in the Rust code keeps them equal).
$openRouterCatalog = @'
{
  "models": [
    {
      "apply_patch_tool_type": "freeform",
      "base_instructions": "",
      "context_window": 262144,
      "default_reasoning_level": "low",
      "default_reasoning_summary": "none",
      "description": "Vision + computer-use. Verified: image answered correctly, tool call completed.",
      "display_name": "Nex N2.5 Pro (free)",
      "effective_context_window_percent": 95,
      "experimental_supported_tools": [],
      "input_modalities": [
        "text",
        "image"
      ],
      "max_context_window": 262144,
      "priority": 1,
      "shell_type": "shell_command",
      "slug": "nex-agi/nex-n2.5-pro:free",
      "support_verbosity": false,
      "supported_in_api": true,
      "supported_reasoning_levels": [
        {
          "effort": "low",
          "description": "Light reasoning"
        },
        {
          "effort": "medium",
          "description": "Balanced reasoning"
        },
        {
          "effort": "high",
          "description": "Enhanced reasoning"
        }
      ],
      "supports_parallel_tool_calls": true,
      "supports_reasoning_summaries": true,
      "truncation_policy": {
        "limit": 10000,
        "mode": "bytes"
      },
      "visibility": "list"
    },
    {
      "apply_patch_tool_type": "freeform",
      "base_instructions": "",
      "context_window": 1000000,
      "default_reasoning_level": "low",
      "default_reasoning_summary": "none",
      "description": "1M context, tools, no vision. Large-diff and architecture review.",
      "display_name": "Nemotron 3 Ultra (free)",
      "effective_context_window_percent": 95,
      "experimental_supported_tools": [],
      "input_modalities": [
        "text"
      ],
      "max_context_window": 1000000,
      "priority": 2,
      "shell_type": "shell_command",
      "slug": "nvidia/nemotron-3-ultra-550b-a55b:free",
      "support_verbosity": false,
      "supported_in_api": true,
      "supported_reasoning_levels": [
        {
          "effort": "low",
          "description": "Light reasoning"
        },
        {
          "effort": "medium",
          "description": "Balanced reasoning"
        },
        {
          "effort": "high",
          "description": "Enhanced reasoning"
        }
      ],
      "supports_parallel_tool_calls": true,
      "supports_reasoning_summaries": true,
      "truncation_policy": {
        "limit": 10000,
        "mode": "bytes"
      },
      "visibility": "list"
    },
    {
      "apply_patch_tool_type": "freeform",
      "base_instructions": "",
      "context_window": 262144,
      "default_reasoning_level": "low",
      "default_reasoning_summary": "none",
      "description": "Vision cross-check, separate vendor from Nex. Keep reasoning effort low.",
      "display_name": "Ling 3.0 Flash VL (free)",
      "effective_context_window_percent": 95,
      "experimental_supported_tools": [],
      "input_modalities": [
        "text",
        "image"
      ],
      "max_context_window": 262144,
      "priority": 3,
      "shell_type": "shell_command",
      "slug": "inclusionai/ling-3.0-flash-vl:free",
      "support_verbosity": false,
      "supported_in_api": true,
      "supported_reasoning_levels": [
        {
          "effort": "low",
          "description": "Light reasoning"
        },
        {
          "effort": "medium",
          "description": "Balanced reasoning"
        },
        {
          "effort": "high",
          "description": "Enhanced reasoning"
        }
      ],
      "supports_parallel_tool_calls": true,
      "supports_reasoning_summaries": true,
      "truncation_policy": {
        "limit": 10000,
        "mode": "bytes"
      },
      "visibility": "list"
    }
  ]
}
'@

function Fail($message) {
  [Console]::Error.WriteLine("エラー: $message")
  exit 1
}

function Step($message) { Write-Output "▶ $message" }

function Write-Utf8($path, $text) {
  $tmp = "$path.codex-switch.tmp"
  [IO.File]::WriteAllText($tmp, $text, (New-Object Text.UTF8Encoding $false))
  Move-Item -Force $tmp $path
}

function Has-ManagedBlock($text) {
  $s = $text.IndexOf($managedBegin); $e = $text.IndexOf($managedEnd)
  $s -ge 0 -and $e -gt $s
}

function TomlString($s) { '"' + $s.Replace('\', '\\').Replace('"', '\"') + '"' }

function KeyFile($name) { Join-Path $keyDir $providers[$name].Key }
function CatalogFile($name) { Join-Path $sidecarHome $providers[$name].Catalog }
function HasKey($name) { $f = KeyFile $name; (Test-Path $f) -and (Get-Item $f).Length -gt 0 }
function KeyHint($name) { "Set-Content -NoNewline -Path '$(KeyFile $name)' -Value '<キー>'   （PowerShell）" }

function ManagedBlock($name, $model) {
  $p = $providers[$name]
  "$managedBegin`n" +
  "model_provider = $(TomlString $name)`n" +
  "model = $(TomlString $model)`n" +
  "model_catalog_json = $(TomlString (CatalogFile $name))`n" +
  "model_reasoning_effort = $(TomlString $p.Effort)`n" +
  $managedEnd
}

# The key is read from its file by a command, so no secret is written into config.toml.
function ProviderTables($name) {
  $p = $providers[$name]
  $cmd = Join-Path $env:SystemRoot 'System32\cmd.exe'
  "[model_providers.$name]`n" +
  "name = $(TomlString $p.Label)`n" +
  "base_url = $(TomlString $p.BaseUrl)`n" +
  "wire_api = `"responses`"`n" +
  "requires_openai_auth = false`n`n" +
  "[model_providers.$name.auth]`n" +
  "command = $(TomlString $cmd)`n" +
  "args = [`"/c`", `"type`", $(TomlString (KeyFile $name))]`n"
}

function NeedsFetch($name) { -not (Test-Path (CatalogFile $name)) -and $name -ne 'openrouter' -and $providers[$name].CatalogUrl }

# Creates a missing model catalog: OpenRouter's from the copy above, Z.ai's
# from Z.ai (which needs the key). Z.ai lists fewer reasoning levels than its
# models accept, so the app's Effort slider would show only two steps.
function Ensure-Catalog($name) {
  $path = CatalogFile $name
  if (Test-Path $path) { return 'Present' }
  New-Item -ItemType Directory -Force $sidecarHome | Out-Null
  if ($name -eq 'openrouter') { Write-Utf8 $path "$openRouterCatalog`n"; return 'Bundled' }
  $url = $providers[$name].CatalogUrl
  if (-not $url) { throw "$($providers[$name].Label) のモデル一覧がありません: $path" }
  $key = ([IO.File]::ReadAllText((KeyFile $name))).Trim()
  [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
  $r = Invoke-WebRequest -UseBasicParsing -TimeoutSec 60 -Uri $url -Headers @{ Authorization = "Bearer $key" }
  $d = [Text.Encoding]::UTF8.GetString($r.RawContentStream.ToArray()) | ConvertFrom-Json
  if (-not $d.models) { throw 'モデル一覧の形式が不正です（models がありません）' }
  $levels = @(
    [pscustomobject]@{ effort = 'low'; description = 'Light reasoning' },
    [pscustomobject]@{ effort = 'medium'; description = 'Balanced reasoning' },
    [pscustomobject]@{ effort = 'high'; description = 'Enhanced reasoning' },
    [pscustomobject]@{ effort = 'xhigh'; description = 'Extended reasoning' },
    [pscustomobject]@{ effort = 'max'; description = 'Deep reasoning' })
  foreach ($m in $d.models) {
    $m | Add-Member -Force -NotePropertyName supported_reasoning_levels -NotePropertyValue $levels
    $m | Add-Member -Force -NotePropertyName default_reasoning_level -NotePropertyValue 'high'
  }
  Write-Utf8 $path ($d | ConvertTo-Json -Depth 50)
  'Fetched'
}

# --- init -----------------------------------------------------------------
function Init-Config {
  $shown = $config.Replace($env:USERPROFILE, '%USERPROFILE%')
  if (-not (Test-Path $config)) {
    $text = "# Codex アプリの第2インスタンス（codexSwitch）の設定。`n" +
      "# 目印の2行で囲んだ管理ブロックは、codexSwitch が起動のたびに書き換えます。`n`n" +
      (ManagedBlock 'zai' $providers.zai.Model) + "`n"
    foreach ($name in $providers.Keys) { $text += "`n" + (ProviderTables $name) }
    Write-Utf8 $config $text
    Write-Output "    作成しました: $shown"
    return
  }
  $text = [IO.File]::ReadAllText($config)
  if (-not (Has-ManagedBlock $text)) { Fail $notOurs }
  $changes = @()
  foreach ($name in $providers.Keys) {
    if ($text -match "(?m)^\s*\[model_providers\.`"?$name`"?\]\s*$") { continue }
    if (-not $text.EndsWith("`n")) { $text += "`n" }
    $text += "`n" + (ProviderTables $name)
    $changes += "[model_providers.$name] を追加"
  }
  if (-not $changes) { Write-Output "    変更なし: $shown"; return }
  $backup = "$config.bak.$(Get-Date -Format 'yyyyMMdd-HHmmss')"
  Copy-Item $config $backup
  Write-Utf8 $config $text
  Write-Output "    更新しました: $shown（$($changes -join '、')）"
  Write-Output "    元のファイル: $($backup.Replace($env:USERPROFILE, '%USERPROFILE%'))"
}

function Init {
  Step "第2インスタンスの設定を用意しています（$($sidecarHome.Replace($env:USERPROFILE, '%USERPROFILE%'))）"
  New-Item -ItemType Directory -Force $sidecarHome | Out-Null
  Init-Config

  if ([Environment]::UserInteractive -and -not [Console]::IsInputRedirected) {
    Write-Output ''
    Step 'API キーを貼り付けて Enter を押してください（画面には表示されません）。使わないもの、あとでファイルに入れるものは、そのまま Enter'
    foreach ($name in $providers.Keys) {
      if (HasKey $name) { continue }
      $secure = Read-Host -AsSecureString -Prompt "  $($providers[$name].Label)"
      $bstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)
      try { $key = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($bstr).Trim() }
      finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr) }
      if (-not $key) { continue }
      New-Item -ItemType Directory -Force $keyDir | Out-Null
      [IO.File]::WriteAllText((KeyFile $name), $key, (New-Object Text.UTF8Encoding $false))
      Write-Output "    保存しました: $(KeyFile $name)"
    }
  }

  Write-Output ''
  Step 'モデル一覧を用意しています'
  foreach ($name in $providers.Keys) {
    $label = '{0,-16}' -f $providers[$name].Label
    $shown = (CatalogFile $name).Replace($env:USERPROFILE, '%USERPROFILE%')
    if ((NeedsFetch $name) -and -not (HasKey $name)) { Write-Output "    $label : キーを入れたあと、最初の起動で取得します"; continue }
    try {
      $state = @{ Present = 'あり'; Bundled = '作成しました'; Fetched = '取得しました' }[(Ensure-Catalog $name)]
      Write-Output "    $label : $state（$shown）"
    } catch { Write-Output "    $label : 取得できませんでした。最初の起動でもう一度取得します（$($_.Exception.Message)）" }
  }

  Write-Output ''
  Step '準備ができました'
  Write-Output ''
  Write-Output 'API キーのファイル（使うものだけで構いません）:'
  foreach ($name in $providers.Keys) {
    $label = '{0,-16}' -f $providers[$name].Label
    if (HasKey $name) { Write-Output "  $label $(KeyFile $name)  ✓" }
    else {
      Write-Output "  $label $(KeyFile $name)  未設定"
      Write-Output "  $('{0,-16}' -f '') 入れ方: $(KeyHint $name)"
    }
  }
  Write-Output ''
  Write-Output '起動:'
  foreach ($name in $providers.Keys) { Write-Output "  codexSwitch -$name" }
}

# --- launch ---------------------------------------------------------------
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

function Launch($provider, $model) {
  if ((Get-Process -Id $PID).SessionId -eq 0) {
    Fail 'サインイン中のデスクトップのターミナルから実行してください。SSH などから起動すると、アプリは画面に表示されません'
  }
  if (-not (Test-Path $config)) { Fail "第2インスタンスの設定がありません。先に codexSwitch init を実行してください" }
  $text = [IO.File]::ReadAllText($config)
  $start = $text.IndexOf($managedBegin)
  $end = $text.IndexOf($managedEnd)
  if (-not (Has-ManagedBlock $text)) { Fail $notOurs }
  $block = $text.Substring($start, $end - $start)
  $active = @{}
  foreach ($k in 'model_provider', 'model') {
    if ($block -match "(?m)^$k\s*=\s*[""']([^""']*)[""']") { $active[$k] = $Matches[1] }
  }

  $name = if ($provider) { $provider } else { $active['model_provider'] }
  if (-not $providers.Contains([string]$name)) { Fail "-$name は使えません。-zai か -openrouter を指定してください" }
  $p = $providers[$name]
  # Without a provider, start exactly what ran last time.
  if (-not $model) { $model = if (-not $provider -and $active['model']) { $active['model'] } else { $p.Model } }

  # Only the provider being started is checked: an unused one is no concern.
  if (-not (HasKey $name)) {
    Fail ("$($p.Label) の API キーが入っていません。次のファイルにキーを入れてから、もう一度実行してください`n" +
      "  ファイル: $(KeyFile $name)`n  入れ方  : $(KeyHint $name)`n" +
      '  （codexSwitch init を実行すると、キーを貼り付けて保存することもできます）')
  }
  if (NeedsFetch $name) { Step "$($p.Label) からモデル一覧を取得しています" }
  try { $state = Ensure-Catalog $name } catch { Fail "モデル一覧を用意できませんでした: $($_.Exception.Message)" }
  if ($state -eq 'Bundled') { Step "モデル一覧を作成しました: $(CatalogFile $name)" }
  if ($state -eq 'Fetched') { Step "モデル一覧を取得しました: $(CatalogFile $name)" }

  # A running instance is never stopped here: quitting is left to the app's UI.
  if (Running) {
    if ($name -eq $active['model_provider'] -and $model -eq $active['model']) {
      # The app allows one instance per user-data dir: the new process hands
      # over to the running one, which then shows its window.
      Start-Sidecar
      Step "第2インスタンスは $name / $model で起動中です。ウィンドウを表示しました"
      return
    }
    Fail ("第2インスタンスが $($active['model_provider']) / $($active['model']) で起動中です。-zai / -openrouter とモデルは起動時に読まれるため、" +
      "アプリを終了してから、もう一度実行してください`n  終了のしかた: $quitHowto")
  }

  # The provider is read at startup, so it is written before launching.
  $text = $text.Substring(0, $start) + (ManagedBlock $name $model) + $text.Substring($end + $managedEnd.Length)
  Write-Utf8 $config $text

  Start-Sidecar
  $deadline = (Get-Date).AddSeconds(30)
  while (-not (Running)) {
    if ((Get-Date) -gt $deadline) { Fail '第2インスタンスの起動を30秒以内に確認できませんでした' }
    Start-Sleep -Milliseconds 500
  }
  Step "第2インスタンスを $name / $model で起動しました"
}

# --- arguments ------------------------------------------------------------
if ($args.Count -ge 1 -and [string]$args[0] -eq 'init') {
  if ($args.Count -gt 1) { Fail 'init は引数を取りません' }
  Init
  exit 0
}
$provider = $null
$model = $null
for ($i = 0; $i -lt $args.Count; $i++) {
  $arg = [string]$args[$i]
  if ($arg -in '--model', '-model') {
    if ($i + 1 -ge $args.Count) { Fail '--model のあとにモデル名を指定してください' }
    $i++
    $model = [string]$args[$i]
  } elseif ($arg -in '-h', '--help', '/?') {
    Get-Content $PSCommandPath -Encoding UTF8 | Select-Object -Skip 2 -First 5 | ForEach-Object { $_.Substring(2) }
    exit 0
  } elseif ($arg -match '^-([A-Za-z0-9][A-Za-z0-9_-]+)$' -and -not $provider) {
    $provider = $Matches[1].ToLower()
  } elseif ($arg -in 'handoff', 'status') {
    Fail "$arg には codexSwitch.exe が必要です。このスクリプト版は準備（init）と起動だけを行います"
  } else {
    Fail "不明な引数です: $arg（使い方: codexSwitch init | codexSwitch [-zai | -openrouter] [--model <slug>]）"
  }
}
if ($provider -and -not $providers.Contains($provider)) { Fail "-$provider は使えません。-zai か -openrouter を指定してください" }
Launch $provider $model
