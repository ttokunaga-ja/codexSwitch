# codexSwitch のスクリプト版（Windows）: Codex アプリの第2インスタンスを準備・起動し、会話を引き継ぐ
#
#   codexSwitch init                                    最初の準備（設定とモデル一覧を作り、API キーの置き場所を案内する）
#   codexSwitch                                         前回と同じ内容で起動
#   codexSwitch -zai / -openrouter                      Z.ai / OpenRouter で起動（--model <slug> でモデルも指定できる）
#   codexSwitch handoff <チャット名/ID> [-zai | -openrouter]   本体の会話を引き継ぐ（チャット名は引用符なしでよい）
#
# codexSwitch.exe と同じ動作をする。スマート アプリ コントロールなどで
# 署名のない exe を実行できない Windows 向け。codexSwitch.cmd から呼ばれる。
# 状態表示（status）は exe だけ。設定ファイル（~/.config/codex-switch/config.toml）は読まず、既定値で動く。
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

# Progress goes straight to the host, so functions that return values stay clean.
function Step($message) { Write-Host "▶ $message" }
function Say($message) { Write-Host $message }

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

function Get-CodexPackage {
  $pkg = Get-AppxPackage OpenAI.Codex | Sort-Object Version -Descending | Select-Object -First 1
  if (-not $pkg) { Fail 'Codex アプリ（パッケージ OpenAI.Codex）が見つかりません' }
  $pkg
}

# The package has no execution alias, so cmd.exe is run inside it; cmd sets
# the variables and starts the app, which inherits them.
function Start-Sidecar {
  $pkg = Get-CodexPackage
  $exe = Join-Path $pkg.InstallLocation 'app\ChatGPT.exe'
  New-Item -ItemType Directory -Force $userData | Out-Null
  $cmdArgs = "/c set ""CODEX_HOME=$sidecarHome"" && set ""CODEX_ELECTRON_USER_DATA_PATH=$userData"" " +
    "&& start """" ""$exe"" ""--user-data-dir=$userData"""
  Invoke-CommandInDesktopPackage -PackageFamilyName $pkg.PackageFamilyName -AppId App `
    -Command 'cmd.exe' -Args $cmdArgs
}

function Assert-Desktop {
  if ((Get-Process -Id $PID).SessionId -eq 0) {
    Fail 'サインイン中のデスクトップのターミナルから実行してください。SSH などから起動すると、アプリは画面に表示されません'
  }
}

# What the managed block says: what the sidecar runs, or ran last.
function Get-Active {
  if (-not (Test-Path $config)) { Fail "第2インスタンスの設定がありません。先に codexSwitch init を実行してください" }
  $text = [IO.File]::ReadAllText($config)
  if (-not (Has-ManagedBlock $text)) { Fail $notOurs }
  $block = $text.Substring($text.IndexOf($managedBegin), $text.IndexOf($managedEnd) - $text.IndexOf($managedBegin))
  $active = @{ model_provider = ''; model = '' }
  foreach ($k in 'model_provider', 'model') {
    if ($block -match "(?m)^$k\s*=\s*[""']([^""']*)[""']") { $active[$k] = $Matches[1] }
  }
  $active
}

# The provider is read at startup, so it is written before launching.
function Set-Active($name, $model) {
  $text = [IO.File]::ReadAllText($config)
  $start = $text.IndexOf($managedBegin); $end = $text.IndexOf($managedEnd)
  Write-Utf8 $config ($text.Substring(0, $start) + (ManagedBlock $name $model) + $text.Substring($end + $managedEnd.Length))
}

# Without -zai / -openrouter, exactly what ran last time; with one, its default model.
function Resolve-Target($provider, $active) {
  $name = if ($provider) { $provider } else { $active['model_provider'] }
  if (-not $providers.Contains([string]$name)) { Fail "-$name は使えません。-zai か -openrouter を指定してください" }
  $model = if (-not $provider -and $active['model']) { $active['model'] } else { $providers[$name].Model }
  $name, $model
}

# Only the target being started is checked: an unused one is no concern.
function Assert-Ready($name) {
  $p = $providers[$name]
  if (-not (HasKey $name)) {
    Fail ("$($p.Label) の API キーが入っていません。次のファイルにキーを入れてから、もう一度実行してください`n" +
      "  ファイル: $(KeyFile $name)`n  入れ方  : $(KeyHint $name)`n" +
      '  （codexSwitch init を実行すると、キーを貼り付けて保存することもできます）')
  }
  if (NeedsFetch $name) { Step "$($p.Label) からモデル一覧を取得しています" }
  try { $state = Ensure-Catalog $name } catch { Fail "モデル一覧を用意できませんでした: $($_.Exception.Message)" }
  if ($state -eq 'Bundled') { Step "モデル一覧を作成しました: $(CatalogFile $name)" }
  if ($state -eq 'Fetched') { Step "モデル一覧を取得しました: $(CatalogFile $name)" }
}

function Start-AndWait($name, $model) {
  Set-Active $name $model
  Start-Sidecar
  $deadline = (Get-Date).AddSeconds(30)
  while (-not (Running)) {
    if ((Get-Date) -gt $deadline) { throw '第2インスタンスの起動を30秒以内に確認できませんでした' }
    Start-Sleep -Milliseconds 500
  }
}

function Launch($provider, $model) {
  Assert-Desktop
  $active = Get-Active
  $name, $default = Resolve-Target $provider $active
  if (-not $model) { $model = $default }
  Assert-Ready $name

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
  try { Start-AndWait $name $model } catch { Fail $_.Exception.Message }
  Step "第2インスタンスを $name / $model で起動しました"
}

# --- handoff --------------------------------------------------------------
$sourceHome = Join-Path $env:USERPROFILE '.codex'
$cacheDir = Join-Path $env:LOCALAPPDATA 'codex-switch'

# PowerShell has no SQLite of its own; Windows ships one (winsqlite3.dll).
# The thread database is only ever opened read-only.
$sqliteSource = @'
using System; using System.Collections.Generic; using System.Runtime.InteropServices; using System.Text;
public static class CodexSwitchSqlite {
  const string Dll = "winsqlite3.dll";
  [DllImport(Dll, CallingConvention = CallingConvention.Winapi)] static extern int sqlite3_open_v2(byte[] filename, out IntPtr db, int flags, IntPtr vfs);
  [DllImport(Dll, CallingConvention = CallingConvention.Winapi)] static extern int sqlite3_close(IntPtr db);
  [DllImport(Dll, CallingConvention = CallingConvention.Winapi)] static extern int sqlite3_prepare16_v2(IntPtr db, [MarshalAs(UnmanagedType.LPWStr)] string sql, int nByte, out IntPtr stmt, IntPtr tail);
  [DllImport(Dll, CallingConvention = CallingConvention.Winapi)] static extern int sqlite3_bind_text16(IntPtr stmt, int index, [MarshalAs(UnmanagedType.LPWStr)] string value, int nByte, IntPtr destructor);
  [DllImport(Dll, CallingConvention = CallingConvention.Winapi)] static extern int sqlite3_step(IntPtr stmt);
  [DllImport(Dll, CallingConvention = CallingConvention.Winapi)] static extern int sqlite3_column_count(IntPtr stmt);
  [DllImport(Dll, CallingConvention = CallingConvention.Winapi)] static extern int sqlite3_column_type(IntPtr stmt, int col);
  [DllImport(Dll, CallingConvention = CallingConvention.Winapi)] static extern IntPtr sqlite3_column_text16(IntPtr stmt, int col);
  [DllImport(Dll, CallingConvention = CallingConvention.Winapi)] static extern int sqlite3_finalize(IntPtr stmt);
  [DllImport(Dll, CallingConvention = CallingConvention.Winapi)] static extern IntPtr sqlite3_errmsg16(IntPtr db);

  public static List<string[]> Query(string path, string sql, string[] args) {
    IntPtr db;
    if (sqlite3_open_v2(Encoding.UTF8.GetBytes(path + "\0"), out db, 1 /* read-only */, IntPtr.Zero) != 0) {
      string m = Error(db); sqlite3_close(db);
      throw new Exception("DB を開けません: " + path + ": " + m);
    }
    try {
      IntPtr st;
      if (sqlite3_prepare16_v2(db, sql, -1, out st, IntPtr.Zero) != 0) throw new Exception(Error(db));
      try {
        for (int i = 0; i < args.Length; i++) sqlite3_bind_text16(st, i + 1, args[i], -1, new IntPtr(-1));
        var rows = new List<string[]>();
        int n = sqlite3_column_count(st);
        while (true) {
          int rc = sqlite3_step(st);
          if (rc == 101) break;                        // SQLITE_DONE
          if (rc != 100) throw new Exception(Error(db)); // not SQLITE_ROW
          var row = new string[n];
          for (int c = 0; c < n; c++)
            row[c] = sqlite3_column_type(st, c) == 5 ? "" : Marshal.PtrToStringUni(sqlite3_column_text16(st, c));
          rows.Add(row);
        }
        return rows;
      } finally { sqlite3_finalize(st); }
    } finally { sqlite3_close(db); }
  }

  static string Error(IntPtr db) {
    return db == IntPtr.Zero ? "out of memory" : Marshal.PtrToStringUni(sqlite3_errmsg16(db));
  }
}
'@

# JSON through .NET: ConvertFrom-Json rejects keys that differ only in case,
# which the app's messages contain.
Add-Type -AssemblyName System.Web.Extensions
$json = New-Object System.Web.Script.Serialization.JavaScriptSerializer
$json.MaxJsonLength = [int]::MaxValue
$json.RecursionLimit = 1000

# Walks nested JSON objects; $null when any key is missing. The parser's
# dictionaries have ContainsKey only: PowerShell cannot call their Contains.
function JGet($o) {
  foreach ($k in $args) {
    if ($o -is [Collections.IDictionary] -and $o.ContainsKey($k)) { $o = $o[$k] } else { return $null }
  }
  , $o
}

function Strip-Verbatim($path) {
  if ($path.StartsWith('\\?\UNC\')) { return '\\' + $path.Substring(8) }
  if ($path.StartsWith('\\?\')) { return $path.Substring(4) }
  $path
}

function Tilde($path) {
  $p = Strip-Verbatim ([string]$path)
  if ($p.StartsWith($env:USERPROFILE, [StringComparison]::OrdinalIgnoreCase)) { return '~' + $p.Substring($env:USERPROFILE.Length) }
  $p
}

# Newest state_<N>.sqlite, so a schema bump does not break lookups.
function State-Db($codexHome) {
  $db = Get-ChildItem $codexHome -File -Filter 'state_*.sqlite' -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -match '^state_\d+\.sqlite$' } |
    Sort-Object { [int]($_.Name -replace '\D', '') } -Descending | Select-Object -First 1
  if (-not $db) { Fail "state_*.sqlite が見つかりません: $codexHome" }
  if (-not ('CodexSwitchSqlite' -as [type])) { Add-Type -TypeDefinition $sqliteSource }
  $db.FullName
}

$threadColumns = "id, coalesce(name, ''), coalesce(preview, ''), coalesce(title, ''), cwd, coalesce(model, ''), " +
  "coalesce(model_provider, ''), rollout_path, coalesce(updated_at_ms, updated_at * 1000, 0)"

function To-Thread($r) {
  [pscustomobject]@{ Id = $r[0]; Name = $r[1]; Preview = $r[2]; Title = $r[3]; Cwd = $r[4]; Model = $r[5]
    Provider = $r[6]; Rollout = $r[7]; Updated = [long]$r[8] }
}

function Thread-ById($db, $id) {
  $rows = [CodexSwitchSqlite]::Query($db, "select $threadColumns from threads where id = ?1", [string[]]@($id))
  if ($rows.Count) { To-Thread $rows[0] }
}

# An ID, or part of a chat name. Names are searched first; only if none match
# does the search widen to the opening message. Sub-agent threads (whose source
# is a JSON object) and archived threads are skipped.
function Search-Threads($db, $query) {
  if ($query -match '^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$') {
    return @(Thread-ById $db $query)
  }
  $pattern = '%' + $query.Replace('\', '\\').Replace('%', '\%').Replace('_', '\_') + '%'
  foreach ($condition in "name like ?1 escape '\'", "preview like ?1 escape '\' or title like ?1 escape '\'") {
    $sql = "select $threadColumns from threads where archived = 0 and source not like '{%' and ($condition) " +
      'order by updated_at_ms desc limit 20'
    $rows = [CodexSwitchSqlite]::Query($db, $sql, [string[]]@($pattern))
    if ($rows.Count) { return @($rows | ForEach-Object { To-Thread $_ }) }
  }
  @()
}

# One-line label: the name, or else the first line of the opening message.
function Thread-Label($t) {
  foreach ($s in $t.Name, $t.Preview, $t.Title) {
    foreach ($line in ([string]$s -split "`r?`n")) {
      $l = $line.Trim()
      if ($l) { if ($l.Length -gt 60) { return $l.Substring(0, 60) + '…' } else { return $l } }
    }
  }
  $t.Id
}

function Resolve-Thread($db, $query) {
  $found = @(Search-Threads $db $query)
  if ($found.Count -eq 1) { return $found[0] }
  if (-not $found) { Fail "該当する会話が見つかりません: $query" }
  Say "$($found.Count) 件の会話が該当しました。ID で指定してください。`n"
  foreach ($t in $found) {
    $date = [DateTimeOffset]::FromUnixTimeMilliseconds($t.Updated).LocalDateTime.ToString('yyyy-MM-dd HH:mm')
    Say "  $($t.Id)  $date  $(Thread-Label $t)"
    Say "      $($t.Model) / $($t.Provider) / $(Tilde $t.Cwd)"
  }
  Fail '会話を1つに絞れませんでした'
}

# A fork stores only the turns after the fork point and replays the rest
# from its parent: every ancestor has to be copied along.
function Get-Chain($db, $thread) {
  $paths = @($thread.Rollout)
  $current = $thread.Rollout
  while ($true) {
    $reader = New-Object IO.StreamReader((Strip-Verbatim $current), [Text.Encoding]::UTF8)
    try { $first = $reader.ReadLine() } finally { $reader.Close() }
    $meta = $json.DeserializeObject($first)
    $parentId = if ((JGet $meta 'type') -eq 'session_meta') { JGet $meta 'payload' 'forked_from_id' }
    if (-not $parentId) { return , $paths }
    if ($paths.Count -gt 64) { Fail 'フォーク元のたどりが深すぎます（循環の可能性）' }
    $parent = Thread-ById $db $parentId
    if (-not $parent) { Fail "フォーク元の会話 $parentId が見つかりません" }
    $paths += $parent.Rollout
    $current = $parent.Rollout
  }
}

# Input size of the most recent request: roughly what the first message sends.
function Last-Usage($paths) {
  foreach ($path in $paths) {
    $found = $null
    foreach ($line in [IO.File]::ReadLines((Strip-Verbatim $path))) {
      if (-not $line.Contains('"token_count"')) { continue }
      try { $n = JGet ($json.DeserializeObject($line)) 'payload' 'info' 'last_token_usage' 'input_tokens' } catch { continue }
      if ($null -ne $n) { $found = [long]$n }
    }
    if ($null -ne $found) { return $found }
  }
  $null
}

# Copies a rollout to the same place under the sidecar's home, checked by hash.
function Copy-Rollout($path) {
  $src = Strip-Verbatim $path
  $base = (Strip-Verbatim $sourceHome).TrimEnd('\', '/')
  $inside = $src.Length -gt $base.Length -and $src.Substring(0, $base.Length) -ieq $base -and $src[$base.Length] -in '\', '/'
  if (-not $inside) { throw "会話ファイルが $sourceHome の外にあります: $src" }
  $dst = Join-Path $sidecarHome $src.Substring($base.Length + 1)
  $hash = (Get-FileHash -LiteralPath $src -Algorithm SHA256).Hash
  if ((Test-Path -LiteralPath $dst) -and (Get-FileHash -LiteralPath $dst -Algorithm SHA256).Hash -eq $hash) { return 'AlreadyPresent' }
  New-Item -ItemType Directory -Force (Split-Path $dst) | Out-Null
  $tmp = "$dst.codex-switch.tmp"
  Copy-Item -LiteralPath $src $tmp -Force
  if ((Get-FileHash -LiteralPath $tmp -Algorithm SHA256).Hash -ne $hash) {
    Remove-Item -LiteralPath $tmp -Force
    throw "コピー後のハッシュが一致しません: $src"
  }
  Move-Item -LiteralPath $tmp $dst -Force
  'Copied'
}

# The package's codex.exe cannot run from outside the package, so a copy per
# app version is kept; older copies are removed.
function Get-CodexCopy {
  $pkg = Get-CodexPackage
  $target = Join-Path $cacheDir "codex-$($pkg.Version).exe"
  if (Test-Path $target) { return $target }
  Step 'codex の実行ファイルを準備しています（初回のみ）'
  New-Item -ItemType Directory -Force $cacheDir | Out-Null
  Copy-Item (Join-Path $pkg.InstallLocation 'app\resources\codex.exe') "$target.tmp" -Force
  Move-Item "$target.tmp" $target -Force
  Get-ChildItem $cacheDir -Filter 'codex-*.exe' | Where-Object { $_.FullName -ne $target } | Remove-Item -Force
  $target
}

# Minimal client for `codex app-server` (line-delimited JSON-RPC over stdio),
# the interface the app itself uses.
function Start-AppServer($codex, $cwd) {
  $psi = New-Object Diagnostics.ProcessStartInfo $codex, 'app-server'
  $psi.UseShellExecute = $false
  $psi.CreateNoWindow = $true
  $psi.RedirectStandardInput = $true
  $psi.RedirectStandardOutput = $true
  $psi.RedirectStandardError = $true
  $psi.StandardOutputEncoding = New-Object Text.UTF8Encoding $false
  $psi.EnvironmentVariables['CODEX_HOME'] = $sidecarHome
  $dir = Strip-Verbatim ([string]$cwd)
  if ($dir -and (Test-Path -LiteralPath $dir -PathType Container)) { $psi.WorkingDirectory = $dir }
  $script:server = [Diagnostics.Process]::Start($psi)
  $script:server.BeginErrorReadLine()
  $script:pending = $null
  $script:backlog = New-Object Collections.Queue
  $script:nextId = 1
  Invoke-Rpc 'initialize' @{ clientInfo = @{ name = 'codexSwitch'; version = '0.1.0' } } 60 | Out-Null
}

# Closing stdin lets the server flush and exit; it is killed only as a last resort.
function Stop-AppServer {
  if (-not $script:server) { return }
  try { $script:server.StandardInput.Close() } catch { }
  if (-not $script:server.WaitForExit(5000)) { try { $script:server.Kill() } catch { } }
  $script:server = $null
}

function Send-Rpc($message) {
  $bytes = [Text.Encoding]::UTF8.GetBytes($json.Serialize($message) + "`n")
  $stream = $script:server.StandardInput.BaseStream
  $stream.Write($bytes, 0, $bytes.Length)
  $stream.Flush()
}

function Receive-Rpc($deadline) {
  while ($true) {
    if (-not $script:pending) { $script:pending = $script:server.StandardOutput.ReadLineAsync() }
    $ms = [Math]::Max(0, [Math]::Min(2147483647, ($deadline - (Get-Date)).TotalMilliseconds))
    if (-not $script:pending.Wait([int]$ms)) { return $null }
    $line = $script:pending.Result
    $script:pending = $null
    if ($null -eq $line) { throw 'app-server が予期せず終了しました' }
    try { return $json.DeserializeObject($line) } catch { }
  }
}

# The server may ask things of its own (approvals and the like). None apply to
# an unattended read-only turn, but each must be answered or the server waits.
function Answer-ServerRequest($message) {
  if (-not ($message -is [Collections.IDictionary] -and $message.ContainsKey('id') -and $message.ContainsKey('method'))) { return $false }
  Send-Rpc @{ id = $message['id']; error = @{ code = -32601; message = 'codexSwitch does not handle this request' } }
  $true
}

function Invoke-Rpc($method, $params, $timeoutSec) {
  $id = $script:nextId
  $script:nextId++
  Send-Rpc @{ id = $id; method = $method; params = $params }
  $deadline = (Get-Date).AddSeconds($timeoutSec)
  while ($true) {
    $message = Receive-Rpc $deadline
    if ($null -eq $message) { throw "$method の応答がタイムアウトしました" }
    if (Answer-ServerRequest $message) { continue }
    if (-not $message.ContainsKey('method') -and $message.ContainsKey('id') -and [long]$message['id'] -eq $id) {
      if ($message.ContainsKey('error')) {
        $text = JGet $message 'error' 'message'
        if (-not $text) { $text = $json.Serialize($message['error']) }
        throw "$method が失敗しました: $text"
      }
      return , (JGet $message 'result')
    }
    $script:backlog.Enqueue($message)
  }
}

# Waits for turn/completed on the thread, keeping the agent's last message.
function Wait-Turn($threadId, $timeoutSec) {
  $deadline = (Get-Date).AddSeconds($timeoutSec)
  $out = @{ Status = ''; Error = $null; Reply = ''; Errors = @() }
  while ($true) {
    if ($script:backlog.Count) { $message = $script:backlog.Dequeue() }
    else {
      $message = Receive-Rpc $deadline
      if ($null -eq $message) { throw "ターンの完了待ちがタイムアウトしました（$timeoutSec 秒）" }
    }
    if (Answer-ServerRequest $message) { continue }
    $params = JGet $message 'params'
    if ((JGet $params 'threadId') -ne $threadId) { continue }
    $method = [string](JGet $message 'method')
    if ($method -eq 'item/completed' -and (JGet $params 'item' 'type') -eq 'agentMessage') {
      $text = JGet $params 'item' 'text'
      if ($null -ne $text) { $out.Reply = [string]$text }
    } elseif ($method -eq 'error') {
      $text = JGet $params 'error' 'message'
      if ($text) { $out.Errors += [string]$text }
    } elseif ($method -eq 'turn/completed') {
      $out.Status = [string](JGet $params 'turn' 'status')
      if (-not $out.Status) { $out.Status = 'unknown' }
      $out.Error = JGet $params 'turn' 'error' 'message'
      return $out
    }
  }
}

# Assigns the thread to the local project whose root most closely contains
# its working directory. The app rewrites this file while running, so it is
# only edited while the sidecar is stopped.
function Assign-Project($threadId, $cwd) {
  $path = Join-Path $sidecarHome '.codex-global-state.json'
  # A sidecar that has never been started has no projects yet.
  if (-not (Test-Path $path)) { return $null }
  $state = $json.DeserializeObject([IO.File]::ReadAllText($path))
  $normalize = { param($p) $n = (Strip-Verbatim ($p -replace '/', '\')).ToLowerInvariant().TrimEnd('\'); if ($n) { $n } else { $p.ToLowerInvariant() } }
  $target = & $normalize ([string]$cwd)
  $best = $null
  $projects = JGet $state 'local-projects'
  if ($projects -is [Collections.IDictionary]) {
    foreach ($key in @($projects.Keys)) {
      $project = $projects[$key]
      $id = JGet $project 'id'; if (-not $id) { $id = $key }
      $name = JGet $project 'name'; if (-not $name) { $name = $id }
      foreach ($root in @(JGet $project 'rootPaths')) {
        if ($root -isnot [string]) { continue }
        $r = & $normalize $root
        $within = $target -eq $r -or $target.StartsWith("$r\", [StringComparison]::Ordinal)
        if ($within -and (-not $best -or $r.Length -gt $best.Length)) { $best = @{ Length = $r.Length; Id = $id; Name = $name } }
      }
    }
  }
  if (-not $best) { return $null }
  if (-not $state.ContainsKey('thread-project-assignments')) {
    $state['thread-project-assignments'] = New-Object 'System.Collections.Generic.Dictionary[string,object]'
  }
  $state['thread-project-assignments'][$threadId] = @{ projectKind = 'local'; projectId = $best.Id }
  Copy-Item -LiteralPath $path "$path.codex-switch.bak" -Force
  Write-Utf8 $path $json.Serialize($state)
  $best.Name
}

function Invoke-Handoff($thread, $chain, $name, $model, $newName, $message) {
  Step '会話ファイルをコピーしています'
  foreach ($path in $chain) {
    $copied = Copy-Rollout $path
    Say "    $(if ($copied -eq 'Copied') { 'コピー' } else { '既にあり' }): $(Tilde $path)"
  }
  $codex = Get-CodexCopy

  Step 'フォークしています'
  Start-AppServer $codex $thread.Cwd
  $forked = Invoke-Rpc 'thread/fork' @{ threadId = $thread.Id; modelProvider = $name; model = $model } 300
  $newId = JGet $forked 'thread' 'id'
  if (-not $newId) { throw 'thread/fork の応答にスレッド ID がありません' }
  Invoke-Rpc 'thread/name/set' @{ threadId = $newId; name = $newName } 30 | Out-Null

  # A thread with no messages is hidden from the app's sidebar, so one turn is
  # required. It runs read-only because nobody is watching it.
  Step '最初のメッセージを送っています（読み取り専用）'
  $turnInput = @(@{ type = 'text'; text = $message; text_elements = @() })
  Invoke-Rpc 'turn/start' @{ threadId = $newId; input = $turnInput; sandboxPolicy = @{ type = 'readOnly' } } 60 | Out-Null
  try { $outcome = Wait-Turn $newId 600 } catch { throw "$($_.Exception.Message)`n  作成済みの会話 ID: $newId" }
  foreach ($e in $outcome.Errors) { Say "    （途中のエラー）$e" }
  if ($outcome.Status -ne 'completed') {
    $why = if ($outcome.Error) { $outcome.Error } else { '理由不明' }
    throw "最初のメッセージが完了しませんでした（$($outcome.Status)）: $why`n  作成済みの会話 ID: $newId"
  }
  Say "`n  --- $model の返答 ---"
  foreach ($line in ($outcome.Reply -split "`r?`n")) { Say "  $line" }
  Say '  ---'

  Stop-AppServer

  $project = Assign-Project $newId $thread.Cwd
  if ($project) { Step "プロジェクト「$project」に割り当てました" }
  else { Step "$(Tilde $thread.Cwd) を含むプロジェクトが第2インスタンスにありません（割り当てなし）" }
  $newId
}

function Handoff($query, $provider) {
  Assert-Desktop
  $db = State-Db $sourceHome
  $thread = Resolve-Thread $db $query
  $chain = Get-Chain $db $thread
  $active = Get-Active
  $name, $model = Resolve-Target $provider $active
  Assert-Ready $name

  $label = Thread-Label $thread
  $newName = "$label（$($model.Split('/')[-1])）"
  $message = "$($providers[$name].Label)（$model）へ引き継ぎました。ここまでの状況と、次に着手すべきことを3行以内で整理してください。" +
    'ファイルの変更やコマンドの実行はしないでください。'
  $size = ($chain | ForEach-Object { (Get-Item -LiteralPath (Strip-Verbatim $_)).Length } | Measure-Object -Sum).Sum
  $usage = Last-Usage $chain
  Say "引き継ぎ元   : $label"
  Say "               $($thread.Model) / $($thread.Provider) / $(Tilde $thread.Cwd)"
  Say "               id $($thread.Id)"
  Say "引き継ぎ先   : 第2インスタンス（$(Tilde $sidecarHome)）/ $name / $model"
  Say ("コピー       : 会話ファイル {0} 件（{1:N1} MB）{2}" -f $chain.Count, ($size / 1e6), $(if ($chain.Count -gt 1) { '※フォーク元を含む' } else { '' }))
  Say '最初のメッセージ（読み取り専用で実行）:'
  Say "    $message"
  if ($null -ne $usage) { Say ("送信する量   : 約 {0:N0} トークン（元の会話の直近の入力量）" -f $usage) } else { Say '送信する量   : 不明' }
  Say ''
  if ((Read-Host '続行しますか？ [y/N]') -notmatch '^\s*(y|yes)\s*$') { Say '中止しました。'; return }

  # The project assignment is written while the app is closed, and a new
  # conversation shows up only after a restart: the sidecar has to quit.
  if (Running) {
    Say "`n第2インスタンスをアプリの画面から終了してください。終了を確認したら続けます（Ctrl+C で中止）。"
    Say "  終了のしかた: $quitHowto"
    Step '第2インスタンスの終了を待っています'
    $deadline = (Get-Date).AddSeconds(600)
    while (Running) {
      if ((Get-Date) -gt $deadline) { Fail '第2インスタンスの終了を10分以内に確認できなかったため、中止しました（何も変更していません）' }
      Start-Sleep -Milliseconds 500
    }
    Start-Sleep -Milliseconds 500
  }

  $newId = $null; $failure = $null
  try { $newId = Invoke-Handoff $thread $chain $name $model $newName $message }
  catch { $failure = $_.Exception.Message }
  finally { Stop-AppServer }

  # Relaunch even on failure so the sidecar is never left closed. On failure,
  # go back to what it was running before.
  $back = $failure -and $providers.Contains([string]$active['model_provider'])
  $relaunchName = if ($back) { $active['model_provider'] } else { $name }
  $relaunchModel = if ($back) { $active['model'] } else { $model }
  Step "第2インスタンスを -$relaunchName で起動しています"
  try { Start-AndWait $relaunchName $relaunchModel }
  catch {
    [Console]::Error.WriteLine("  起動に失敗しました: $($_.Exception.Message)")
    [Console]::Error.WriteLine("  手動で起動してください: codexSwitch -$relaunchName")
  }
  if ($failure) { Fail $failure }
  Say "`n完了しました。"
  Say "  会話: $newName"
  Say "  ID  : $newId"
}

# --- arguments ------------------------------------------------------------
$command = if ($args.Count) { [string]$args[0] } else { '' }
if ($command -eq 'init') {
  if ($args.Count -gt 1) { Fail 'init は引数を取りません' }
  Init
  exit 0
}
if ($command -eq 'handoff') {
  # The chat name may be written without quotes: every word that is not
  # -zai / -openrouter belongs to it. After --, every word does.
  $provider = $null
  $words = @()
  $literal = $false
  foreach ($arg in @($args | Select-Object -Skip 1)) {
    $arg = [string]$arg
    if (-not $literal -and $arg -eq '--') { $literal = $true; continue }
    if (-not $literal -and $arg -match '^-([A-Za-z0-9][A-Za-z0-9_-]+)$') {
      if ($provider) { Fail '-zai / -openrouter は1つだけ指定してください' }
      $provider = $Matches[1].ToLower()
      if (-not $providers.Contains($provider)) { Fail "-$provider は使えません。-zai か -openrouter を指定してください" }
      continue
    }
    $words += $arg
  }
  if (-not $words) { Fail '引き継ぐチャット名か ID を指定してください（使い方: codexSwitch handoff <チャット名/ID> [-zai | -openrouter]）' }
  Handoff ($words -join ' ') $provider
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
    Get-Content $PSCommandPath -Encoding UTF8 | Select-Object -Skip 2 -First 4 | ForEach-Object { $_.Substring(2) }
    exit 0
  } elseif ($arg -match '^-([A-Za-z0-9][A-Za-z0-9_-]+)$' -and -not $provider) {
    $provider = $Matches[1].ToLower()
  } elseif ($arg -eq 'status') {
    Fail 'status には codexSwitch.exe が必要です。このスクリプト版は準備（init）、起動、引き継ぎ（handoff）を行います'
  } else {
    Fail "不明な引数です: $arg（使い方: codexSwitch init | codexSwitch [-zai | -openrouter] [--model <slug>] | codexSwitch handoff <チャット名/ID> [-zai | -openrouter]）"
  }
}
if ($provider -and -not $providers.Contains($provider)) { Fail "-$provider は使えません。-zai か -openrouter を指定してください" }
Launch $provider $model
