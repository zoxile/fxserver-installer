param(
    [ValidateSet('Install', 'Uninstall')][string]$Action,
    [string]$InstallDir
)

# Dot-sourcing only defines functions, allowing tests to supply in-memory keys.
function ConvertTo-FxinsPathIdentity([string]$Value) {
    $value = [Environment]::ExpandEnvironmentVariables($Value.Trim().Trim('"'))
    return $value.Replace('/', '\').TrimEnd('\').ToUpperInvariant()
}

function Get-FxinsPathSnapshot($Key) {
    $exists = $Key.GetValueNames() -contains 'Path'
    $kind = [Microsoft.Win32.RegistryValueKind]::ExpandString
    $value = ''
    if ($exists) {
        $kind = $Key.GetValueKind('Path')
        if ($kind -notin @([Microsoft.Win32.RegistryValueKind]::String, [Microsoft.Win32.RegistryValueKind]::ExpandString)) {
            throw 'PATH is not a string; it has not been changed.'
        }
        $value = $Key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    }
    return [pscustomobject]@{ Exists = $exists; Kind = $kind; Value = $value }
}

function Update-FxinsUserPath($EnvironmentKey, $OwnershipKey, [string]$Action, [string]$InstallDir, [string]$MachinePath = '') {
    $entry = [IO.Path]::GetFullPath($InstallDir).TrimEnd('\')
    if ($entry -match '[;"%\r\n\x00]' -or $entry -notmatch '^(?:[A-Za-z]:\\|\\\\)') {
        throw 'This install directory cannot be represented safely in PATH.'
    }
    $identity = ConvertTo-FxinsPathIdentity $entry
    $hash = [Security.Cryptography.SHA256]::Create()
    try {
        $recordName = [BitConverter]::ToString($hash.ComputeHash([Text.Encoding]::UTF8.GetBytes($identity))).Replace('-', '')
    } finally { $hash.Dispose() }
    $before = Get-FxinsPathSnapshot $EnvironmentKey
    $record = $null
    $stored = $OwnershipKey.GetValue($recordName, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    if ($null -ne $stored) {
        $record = $stored | ConvertFrom-Json -ErrorAction Stop
        if ($record.Version -ne 1 -or (ConvertTo-FxinsPathIdentity $record.Entry) -cne $identity) {
            throw 'Unrecognized PATH ownership record; PATH has not been changed.'
        }
    }
    $parts = $before.Value.Split(';')
    $matches = @(for ($i = 0; $i -lt $parts.Count; $i++) {
        if ((ConvertTo-FxinsPathIdentity $parts[$i]) -ceq $identity) { $i }
    })
    $after = $before.Value
    $existsAfter = $before.Exists
    if ($Action -eq 'Install') {
        # A preexisting user or machine entry is never claimed by this installer.
        if ($matches.Count -gt 0) { return }
        foreach ($part in $MachinePath.Split(';')) {
            if ((ConvertTo-FxinsPathIdentity $part) -ceq $identity) { return }
        }
        $separator = if ($after.Length -eq 0 -or $after.EndsWith(';')) { '' } else { ';' }
        $after += $separator + $entry
        if ($after.Length -ge 32767) { throw 'PATH would exceed the Windows environment variable limit; it has not been changed.' }
        $existsAfter = $true
        $record = [pscustomobject]@{
            Version = 1; Entry = $entry; Before = $before.Value
            Existed = $before.Exists; Installed = $after
        }
    } elseif ($Action -eq 'Uninstall') {
        if ($null -eq $record) { return }
        if ($before.Value -ceq $record.Installed) {
            $after = $record.Before
            $existsAfter = $record.Existed
        } elseif ($matches.Count -eq 1 -and $parts[$matches[0]] -ceq $record.Entry) {
            # Preserve unrelated edits made after installation, including empty entries.
            $after = (@(for ($i = 0; $i -lt $parts.Count; $i++) {
                if ($i -ne $matches[0]) { $parts[$i] }
            }) -join ';')
        }
        # Ambiguous duplicates or reformatted entries are left alone, not guessed at.
    } else { throw 'Unknown PATH operation.' }

    $latest = Get-FxinsPathSnapshot $EnvironmentKey
    if ($latest.Value -cne $before.Value -or $latest.Exists -ne $before.Exists -or $latest.Kind -ne $before.Kind) {
        throw 'PATH changed concurrently; retry the operation.'
    }
    if ($after -cne $before.Value -or $existsAfter -ne $before.Exists) {
        if ($existsAfter) {
            $EnvironmentKey.SetValue('Path', $after, $before.Kind)
        } else {
            $EnvironmentKey.DeleteValue('Path', $false)
        }
    }
    if ($Action -eq 'Install') {
        # Record ownership only after the PATH write succeeds. Failure leaves an
        # unclaimed entry, never a claim against an entry we did not add.
        $OwnershipKey.SetValue($recordName, ($record | ConvertTo-Json -Compress), [Microsoft.Win32.RegistryValueKind]::String)
    } else {
        $OwnershipKey.DeleteValue($recordName, $false)
    }
}

function Invoke-FxinsPathUpdate([string]$Action, [string]$InstallDir) {
    # Serialize this installer's updates. Re-read before writing to detect external edits too.
    $sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    $mutex = [Threading.Mutex]::new($false, "Local\FXServerInstaller.TerminalPath.$sid")
    $locked = $false
    $environmentKey = $null
    $ownershipKey = $null
    $machineKey = $null
    try {
        try { $locked = $mutex.WaitOne(30000) } catch [Threading.AbandonedMutexException] { $locked = $true }
        if (-not $locked) { throw 'Another installer is updating PATH; retry shortly.' }
        $environmentKey = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Environment')
        $ownershipKey = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Software\com.fxserver.installer\TerminalPath')
        $machineKey = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey('SYSTEM\CurrentControlSet\Control\Session Manager\Environment')
        $machinePath = if ($null -ne $machineKey) { (Get-FxinsPathSnapshot $machineKey).Value } else { '' }
        Update-FxinsUserPath $environmentKey $ownershipKey $Action $InstallDir $machinePath
    } finally {
        if ($null -ne $machineKey) { $machineKey.Dispose() }
        if ($null -ne $ownershipKey) { $ownershipKey.Dispose() }
        if ($null -ne $environmentKey) { $environmentKey.Dispose() }
        if ($locked) { $mutex.ReleaseMutex() }
        $mutex.Dispose()
    }
}

if ($MyInvocation.InvocationName -ne '.') {
    $ErrorActionPreference = 'Stop'
    try {
        if (-not $Action -or -not $InstallDir) { throw 'Action and InstallDir are required.' }
        Invoke-FxinsPathUpdate $Action $InstallDir
        exit 0
    } catch {
        [Console]::Error.WriteLine($_.Exception.Message)
        exit 1
    }
}
