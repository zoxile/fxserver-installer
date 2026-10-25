$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
if ($env:GITHUB_ACTIONS -cne 'true' -or $env:RUNNER_ENVIRONMENT -cne 'github-hosted' -or
    $env:RUNNER_OS -cne 'Windows' -or $env:GITHUB_RUN_ID -notmatch '^[1-9][0-9]*$' -or
    $env:FXI_DISPOSABLE_WINDOWS_FIREWALL -cne 'github-hosted-disposable-v1') { throw 'Disposable Windows CI opt-in required.' }
if ($nonce -notmatch '^[a-f0-9]{32}$' -or $tcpPort -lt 50000 -or $tcpPort -gt 55997 -or $udpPort -ne ($tcpPort+1)) { throw 'Invalid fixture identifiers.' }
$env:PSModulePath="$PSHOME\Modules"
Import-Module NetSecurity
$marker="FXI CI fixture $nonce"
$external="fxi-ci-external-$nonce"
$otherPort=$tcpPort+2
$other="fxins-fw-v1-in-tcp-$otherPort"
$managedDescription='Managed by FXServer Installer. FiveM host port rule v1.'
$managedGroup='FXServer Installer - FiveM v1'
$modified="fxins-fw-v1-in-udp-$udpPort"
$targetNames=@("fxins-fw-v1-in-tcp-$tcpPort",$modified,"fxins-fw-v1-out-tcp-$tcpPort","fxins-fw-v1-out-udp-$udpPort")

if ($fixtureOperation -eq 'create') {
    $existing=@(Get-NetFirewallRule -PolicyStore PersistentStore | Where-Object { $_.Name -cin (@($external,$other)+$targetNames) })
    if ($existing.Count) { throw 'Fixture name collision; refusing changes.' }
    New-NetFirewallRule -PolicyStore PersistentStore -Name $external -DisplayName $external -Group $marker -Description $marker `
        -Direction Inbound -Action Allow -Protocol TCP -LocalPort $tcpPort -Profile Any -Enabled True | Out-Null
    New-NetFirewallRule -PolicyStore PersistentStore -Name $other -DisplayName $other -Group $managedGroup -Description $managedDescription `
        -Direction Inbound -Action Allow -Protocol TCP -LocalPort $otherPort -Profile Any -Enabled True | Out-Null
} elseif ($fixtureOperation -eq 'verify') {
    $externalRule=Get-NetFirewallRule -PolicyStore PersistentStore -Name $external
    if ($externalRule.Group -cne $marker -or $externalRule.Description -cne $marker -or
        ($externalRule | Get-NetFirewallPortFilter).LocalPort -ne [string]$tcpPort) { throw 'External fixture rule changed or disappeared.' }
    $otherRule=Get-NetFirewallRule -PolicyStore PersistentStore -Name $other
    if ($otherRule.Group -cne $managedGroup -or $otherRule.Description -cne $managedDescription -or
        ($otherRule | Get-NetFirewallPortFilter).LocalPort -ne [string]$otherPort) { throw 'Unrelated managed rule changed or disappeared.' }
} elseif ($fixtureOperation -eq 'modify') {
    $rule=Get-NetFirewallRule -PolicyStore PersistentStore -Name $modified
    if ($rule.Group -cne $managedGroup -or $rule.Description -cne $managedDescription) { throw 'Unexpected owned fixture.' }
    $rule | Set-NetFirewallRule -Description "FXI CI modified $nonce"
} elseif ($fixtureOperation -eq 'cleanup') {
    $rules=@(Get-NetFirewallRule -PolicyStore PersistentStore | Where-Object { $_.Name -cin (@($external,$other)+$targetNames) })
    foreach ($rule in $rules) {
        if ($rule.Name -ceq $external) {
            if ($rule.Group -cne $marker -or $rule.Description -cne $marker) { throw 'External cleanup ownership mismatch.' }
        } elseif ($rule.Group -cne $managedGroup -or $rule.Description -cnotin @($managedDescription,"FXI CI modified $nonce")) {
            throw 'Managed fixture cleanup ownership mismatch.'
        }
        $rule | Remove-NetFirewallRule
    }
} else { throw 'Unknown fixture operation.' }
