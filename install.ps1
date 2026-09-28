# Installs the latest codexSwitch release into %USERPROFILE%\.local\bin and
# adds that folder to the user's PATH. In PowerShell:
#
#   irm https://raw.githubusercontent.com/ttokunaga-ja/codexSwitch/main/install.ps1 | iex
#
# When Windows refuses to run the exe (Smart App Control), the script version
# (codexSwitch.cmd and codexSwitch-script.ps1) is installed instead.
# After this, `codexSwitch update` keeps it up to date.
#
# Run through iex, so it never calls exit: that would close the user's window.
& {
  $ErrorActionPreference = 'Stop'
  $base = 'https://github.com/ttokunaga-ja/codexSwitch/releases/latest/download'
  $bin = Join-Path $env:USERPROFILE '.local\bin'
  [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

  function Download($name) {
    , (Invoke-WebRequest -UseBasicParsing -TimeoutSec 300 -Uri "$base/$name").RawContentStream.ToArray()
  }
  function Sha256($bytes) {
    $h = [Security.Cryptography.SHA256]::Create()
    try { [BitConverter]::ToString($h.ComputeHash([byte[]]$bytes)).Replace('-', '').ToLowerInvariant() } finally { $h.Dispose() }
  }
  $sums = [Text.Encoding]::UTF8.GetString((Download 'SHA256SUMS'))
  function Fetch($name) {
    $bytes = Download $name
    foreach ($line in $sums -split "`r?`n") {
      if ($line -match '^([0-9a-fA-F]{64})\s+\*?(.+?)\s*$' -and $Matches[2] -eq $name) {
        if ((Sha256 $bytes) -eq $Matches[1].ToLowerInvariant()) { return , $bytes }
      }
    }
    throw "ダウンロードした $name のハッシュが一致しません"
  }
  function Put($name, $bytes) {
    $path = Join-Path $bin $name
    [IO.File]::WriteAllBytes("$path.codex-switch.tmp", $bytes)
    Move-Item -LiteralPath "$path.codex-switch.tmp" $path -Force
    $path
  }

  New-Item -ItemType Directory -Force $bin | Out-Null
  $exe = Join-Path $bin 'codexSwitch.exe'
  $trial = Put 'codexSwitch.new.exe' (Fetch 'codexSwitch-windows-x64.exe')
  $version = $null
  try { $version = & $trial --version 2>$null; if ($LASTEXITCODE -ne 0) { $version = $null } } catch { }
  if ($version -like 'codexSwitch *') {
    Move-Item -LiteralPath $trial $exe -Force
    Write-Host "インストールしました: $exe ($version)"
    if (Test-Path (Join-Path $bin 'codexSwitch.cmd')) {
      Write-Host '  同じフォルダのスクリプト版（codexSwitch.cmd と codexSwitch-script.ps1）は使われなくなります。不要なら削除してください'
    }
  } else {
    Remove-Item -LiteralPath $trial -Force
    Write-Host 'codexSwitch.exe を実行できませんでした（スマート アプリ コントロールに止められた可能性があります）。スクリプト版を入れます'
    foreach ($name in 'codexSwitch-script.ps1', 'codexSwitch.cmd') { Put $name (Fetch $name) | Out-Null }
    Write-Host "インストールしました: $(Join-Path $bin 'codexSwitch.cmd')（スクリプト版）"
    if (Test-Path $exe) {
      Write-Host "  注意: $exe が先に使われます。実行できない古い exe なら削除してください"
    }
  }

  # Read and written raw, so entries such as %USERPROFILE%\... stay unexpanded.
  $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true)
  try {
    $path = [string]$key.GetValue('Path', '', 'DoNotExpandEnvironmentNames')
    $present = $path -split ';' | Where-Object { [Environment]::ExpandEnvironmentVariables($_).TrimEnd('\') -ieq $bin }
    if (-not $present) {
      $key.SetValue('Path', ((@($path.TrimEnd(';')) + $bin) -join ';').TrimStart(';'), 'ExpandString')
      # Setting any variable through .NET makes Windows tell running programs,
      # so terminals opened from now on see the new PATH.
      [Environment]::SetEnvironmentVariable('CODEX_SWITCH_INSTALL', '1', 'User')
      [Environment]::SetEnvironmentVariable('CODEX_SWITCH_INSTALL', $null, 'User')
      Write-Host "  PATH に $bin を追加しました"
    }
  } finally { $key.Close() }
  if (-not (($env:Path -split ';') | Where-Object { $_.TrimEnd('\') -ieq $bin })) { $env:Path += ";$bin" }
  Write-Host ''
  Write-Host '次に実行: codexSwitch init'
}
