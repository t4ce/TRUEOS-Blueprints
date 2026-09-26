# W3Box gateway helper. No administrator access is required.
# Changes ONLY the current user's Warcraft III gateway value (32-bit registry view).
# Backs up the exact original value first. Does NOT install or patch Warcraft/W3L.
[CmdletBinding()]
param(
    [string]$ServerAddress = '__SERVER_ADDRESS__',
    [string]$ServerName = '__SERVER_NAME__',
    [string]$Restore = ''
)
$ErrorActionPreference = 'Stop'
$keyPath = 'Software\Blizzard Entertainment\Warcraft III'
$valueName = 'Battle.net Gateways'
$base = [Microsoft.Win32.RegistryKey]::OpenBaseKey(
    [Microsoft.Win32.RegistryHive]::CurrentUser,
    [Microsoft.Win32.RegistryView]::Registry32)
$key = $null
try {
    if ($Restore) {
        $record = Get-Content -LiteralPath $Restore -Raw | ConvertFrom-Json
        if ($record.KeyPath -ne $keyPath -or $record.ValueName -ne $valueName) {
            throw 'This is not a W3Box Warcraft III gateway backup.'
        }
        $key = $base.CreateSubKey($keyPath)
        if (-not $record.Existed) {
            $key.DeleteValue($valueName, $false)
        } elseif ($record.Kind -eq 'MultiString') {
            $key.SetValue($valueName, [string[]]$record.Data, [Microsoft.Win32.RegistryValueKind]::MultiString)
        } elseif ($record.Kind -eq 'Binary') {
            $key.SetValue($valueName, [Convert]::FromBase64String($record.Data), [Microsoft.Win32.RegistryValueKind]::Binary)
        } else { throw 'Unsupported backup value kind.' }
        Write-Host 'Original gateway value restored.'
        return
    }
    if (-not $ServerAddress -or $ServerAddress.StartsWith('__')) {
        $ServerAddress = Read-Host 'Ubuntu server IPv4 address or DNS name (no port)'
    }
    if (-not $ServerName -or $ServerName.StartsWith('__')) { $ServerName = 'RoC 1.21b Realm' }
    if ($ServerAddress -notmatch '^[A-Za-z0-9][A-Za-z0-9.-]{0,252}$' -or $ServerAddress.Contains('..')) {
        throw 'Use an IPv4 address or DNS hostname only.'
    }
    if ($ServerName -notmatch '^[A-Za-z0-9 ._-]{1,48}$') { throw 'Invalid server name.' }
    if (Get-Process -Name war3,warcraft3,w3l -ErrorAction SilentlyContinue) {
        throw 'Close Warcraft III and W3L before changing gateways.'
    }
    $key = $base.CreateSubKey($keyPath)
    $existed = $key.GetValueNames() -contains $valueName
    $original = $null
    $kind = 'MultiString'
    if ($existed) {
        $kind = $key.GetValueKind($valueName).ToString()
        $original = $key.GetValue($valueName)
        if ($kind -notin @('MultiString','Binary')) { throw 'Unexpected registry value kind; nothing changed.' }
    }
    $recordData = $original
    if ($kind -eq 'Binary' -and $existed) { $recordData = [Convert]::ToBase64String([byte[]]$original) }
    $backup = Join-Path $env:USERPROFILE ('w3box-gateways-' + (Get-Date -Format 'yyyyMMdd-HHmmss-fff') + '.json')
    @{KeyPath=$keyPath; ValueName=$valueName; Existed=$existed; Kind=$kind; Data=$recordData} |
        ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $backup -Encoding UTF8
    $items = @('1001','01')
    if ($existed -and $kind -eq 'MultiString') { $items = @([string[]]$original) }
    elseif ($existed -and $kind -eq 'Binary') {
        $bytes = [byte[]]$original
        $encoding = [Text.Encoding]::ASCII
        if ($bytes.Length -gt 1 -and $bytes[1] -eq 0) { $encoding = [Text.Encoding]::Unicode }
        $items = @($encoding.GetString($bytes).TrimEnd([char]0) -split "`0")
    }
    if ($items.Count -lt 2 -or (($items.Count - 2) % 3) -ne 0) {
        throw "Unexpected gateway format; nothing changed. Backup: $backup"
    }
    $found = $false
    for ($i=2; $i -lt $items.Count; $i+=3) {
        if ($items[$i] -eq $ServerAddress) {
            $items[$i+2] = $ServerName
            $found = $true
        }
    }
    if (-not $found) { $items += @($ServerAddress,'0',$ServerName) }
    $key.SetValue($valueName, [string[]]$items, [Microsoft.Win32.RegistryValueKind]::MultiString)
    Write-Host "Added gateway: $ServerName ($ServerAddress)"
    Write-Host "Original value backup: $backup"
    Write-Host 'Launch your 1.21b-compatible loader and select this gateway in Warcraft III.'
    Write-Host 'This script did NOT install a loader. Modern w3lh/w3l is not suitable for 1.21b.'
} finally {
    if ($null -ne $key) { $key.Dispose() }
    $base.Dispose()
}
