$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. "$PSScriptRoot/../src-tauri/packaging/windows/path.ps1"

# No registry access: these objects implement only the APIs the updater uses.
class MemoryRegistryKey {
    [hashtable]$Values = @{}
    [hashtable]$Kinds = @{}
    [int]$Writes = 0
    [int]$Reads = 0
    [bool]$FailWrite = $false
    [bool]$ChangeOnSecondRead = $false
    [string[]] GetValueNames() { return @($this.Values.Keys) }
    [object] GetValue([string]$name, [object]$fallback, [Microsoft.Win32.RegistryValueOptions]$options) {
        if ($options -ne [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames) { throw 'Unexpected expanding read' }
        $this.Reads++
        if ($this.ChangeOnSecondRead -and $this.Reads -eq 2) { $this.Values['Path'] = 'C:\concurrent-edit' }
        if ($this.Values.ContainsKey($name)) { return $this.Values[$name] }
        return $fallback
    }
    [Microsoft.Win32.RegistryValueKind] GetValueKind([string]$name) { return $this.Kinds[$name] }
    [void] SetValue([string]$name, [object]$value, [Microsoft.Win32.RegistryValueKind]$kind) {
        if ($this.FailWrite) { throw 'Simulated write failure' }
        $this.Values[$name] = $value
        $this.Kinds[$name] = $kind
        $this.Writes++
    }
    [void] DeleteValue([string]$name, [bool]$throwOnMissing) {
        $this.Values.Remove($name)
        $this.Kinds.Remove($name)
        $this.Writes++
    }
}

function Assert-Equal($Expected, $Actual, [string]$Message) {
    if ($Expected -cne $Actual) { throw "$Message (expected '$Expected', got '$Actual')" }
}
function Assert-Throws([scriptblock]$Block, [string]$Message) {
    $threw = $false
    try { & $Block } catch { $threw = $true }
    if (-not $threw) { throw $Message }
}
function New-PathKey([string]$Value, [Microsoft.Win32.RegistryValueKind]$Kind = 'ExpandString') {
    $key = [MemoryRegistryKey]::new()
    $key.Values['Path'] = $Value
    $key.Kinds['Path'] = $Kind
    return $key
}

$install = 'C:\Users\Fixture Name\FXServer Installer'
$longPath = ((1..400 | ForEach-Object { "C:\tools\entry-$_" }) -join ';') + ';%USERPROFILE%\bin;;'
foreach ($initial in @('', 'C:\tools', 'C:\tools;', $longPath)) {
    foreach ($kind in @('String', 'ExpandString')) {
        $environment = New-PathKey $initial $kind
        $ownership = [MemoryRegistryKey]::new()
        Update-FxinsUserPath $environment $ownership Install $install
        $installed = $environment.Values['Path']
        Assert-Equal $true ($installed.EndsWith($install)) 'Installer was not appended'
        Assert-Equal $kind $environment.Kinds['Path'].ToString() 'Registry type changed'
        Update-FxinsUserPath $environment $ownership Install $install
        Assert-Equal $installed $environment.Values['Path'] 'Upgrade duplicated PATH'
        Assert-Equal 1 $environment.Writes 'Upgrade wrote unchanged PATH'
        Update-FxinsUserPath $environment $ownership Uninstall $install
        Assert-Equal $initial $environment.Values['Path'] 'Uninstall did not restore exact original PATH'
        Assert-Equal 0 $ownership.Values.Count 'Uninstall left ownership record'
        Update-FxinsUserPath $environment $ownership Uninstall $install
        Assert-Equal $initial $environment.Values['Path'] 'Repeated uninstall changed PATH'
    }
}
$environment = [MemoryRegistryKey]::new()
$ownership = [MemoryRegistryKey]::new()
Update-FxinsUserPath $environment $ownership Install $install
Update-FxinsUserPath $environment $ownership Uninstall $install
Assert-Equal $false $environment.Values.ContainsKey('Path') 'Absent PATH became an empty registry value'

$env:FXINS_PATH_TEST_ROOT = 'C:\Users\Fixture Name'
foreach ($existing in @($install, ($install.ToLowerInvariant() + '\'), ('"' + $install + '"'), $install.Replace('\', '/'), '%FXINS_PATH_TEST_ROOT%\fixture')) {
    $target = [Environment]::ExpandEnvironmentVariables($existing.Trim('"'))
    $environment = New-PathKey "C:\other;$existing;C:\last"
    $ownership = [MemoryRegistryKey]::new()
    Update-FxinsUserPath $environment $ownership Install $target
    Update-FxinsUserPath $environment $ownership Uninstall $target
    Assert-Equal 0 $environment.Writes 'Preexisting equivalent entry changed'
    Assert-Equal 0 $ownership.Values.Count 'Preexisting entry was claimed'
}
$environment = New-PathKey 'C:\tools'
$ownership = [MemoryRegistryKey]::new()
Update-FxinsUserPath $environment $ownership Install $install $install
Update-FxinsUserPath $environment $ownership Uninstall $install
Assert-Equal 0 $environment.Writes 'Preexisting machine entry changed user PATH'

$environment = New-PathKey 'C:\tools'
$ownership = [MemoryRegistryKey]::new()
Update-FxinsUserPath $environment $ownership Install $install
$environment.Values['Path'] = 'C:\added;' + $environment.Values['Path'] + ';;%USERPROFILE%\later;'
Update-FxinsUserPath $environment $ownership Uninstall $install
Assert-Equal 'C:\added;C:\tools;;%USERPROFILE%\later;' $environment.Values['Path'] 'Later PATH edits were lost'

foreach ($ambiguous in @("C:\tools;$install;$install", ("C:\tools;" + $install.ToUpperInvariant()))) {
    $environment = New-PathKey 'C:\tools'
    $ownership = [MemoryRegistryKey]::new()
    Update-FxinsUserPath $environment $ownership Install $install
    $environment.Values['Path'] = $ambiguous
    Update-FxinsUserPath $environment $ownership Uninstall $install
    Assert-Equal $ambiguous $environment.Values['Path'] 'Ambiguous ownership was guessed'
}

$environment = New-PathKey 'C:\tools'
$ownership = [MemoryRegistryKey]::new()
Update-FxinsUserPath $environment $ownership Install $install
Update-FxinsUserPath $environment $ownership Install 'D:\second install'
Update-FxinsUserPath $environment $ownership Uninstall $install
Assert-Equal 'C:\tools;D:\second install' $environment.Values['Path'] 'Another installation was removed'
Update-FxinsUserPath $environment $ownership Uninstall 'D:\second install'
Assert-Equal 'C:\tools' $environment.Values['Path'] 'Multiple install cleanup failed'

foreach ($bad in @('C:\unsafe;entry', 'C:\unsafe%TEMP%', 'C:\unsafe"entry')) {
    $environment = New-PathKey 'C:\tools'
    $ownership = [MemoryRegistryKey]::new()
    Assert-Throws { Update-FxinsUserPath $environment $ownership Install $bad } 'Unsafe install path accepted'
    Assert-Equal 0 $environment.Writes 'Unsafe path changed PATH'
}
$environment = New-PathKey ('C:\' + ('x' * 32760))
$ownership = [MemoryRegistryKey]::new()
Assert-Throws { Update-FxinsUserPath $environment $ownership Install $install } 'Overlong PATH accepted'
Assert-Equal 0 $environment.Writes 'Overlong PATH was truncated'

$environment = New-PathKey 'C:\tools' 'MultiString'
Assert-Throws { Update-FxinsUserPath $environment $ownership Install $install } 'Non-string registry type accepted'
Assert-Equal 0 $environment.Writes 'Non-string PATH was overwritten'

$environment = New-PathKey 'C:\tools'
$environment.FailWrite = $true
$ownership = [MemoryRegistryKey]::new()
Assert-Throws { Update-FxinsUserPath $environment $ownership Install $install } 'Failed write succeeded'
Assert-Equal 0 $ownership.Values.Count 'Failed PATH write claimed ownership'

$environment = New-PathKey 'C:\tools'
$ownership = [MemoryRegistryKey]::new()
$ownership.FailWrite = $true
Assert-Throws { Update-FxinsUserPath $environment $ownership Install $install } 'Failed ownership write succeeded'
Assert-Equal 0 $ownership.Values.Count 'Failed ownership write created a record'
$unclaimed = $environment.Values['Path']
Update-FxinsUserPath $environment $ownership Uninstall $install
Assert-Equal $unclaimed $environment.Values['Path'] 'Uninstall removed an unclaimed entry'

$environment = New-PathKey 'C:\tools'
$ownership = [MemoryRegistryKey]::new()
Update-FxinsUserPath $environment $ownership Install $install
$ownership.Values[@($ownership.Values.Keys)[0]] = '{invalid fixture'
$unchanged = $environment.Values['Path']
Assert-Throws { Update-FxinsUserPath $environment $ownership Uninstall $install } 'Corrupt ownership was accepted'
Assert-Equal $unchanged $environment.Values['Path'] 'Corrupt ownership changed PATH'

$environment = New-PathKey 'C:\tools'
$environment.ChangeOnSecondRead = $true
$ownership = [MemoryRegistryKey]::new()
Assert-Throws { Update-FxinsUserPath $environment $ownership Install $install } 'Concurrent change was ignored'
Assert-Equal 'C:\concurrent-edit' $environment.Values['Path'] 'Concurrent change was overwritten'
Assert-Equal 0 $environment.Writes 'Concurrent edit triggered a write'

Write-Output 'Windows PATH tests passed using in-memory registry keys only.'
