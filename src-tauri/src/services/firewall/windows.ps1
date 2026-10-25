$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$env:PSModulePath = "$PSHOME\Modules"
Import-Module NetSecurity -ErrorAction Stop
$group = 'FXServer Installer - FiveM v1'
$description = 'Managed by FXServer Installer. FiveM host port rule v1.'

function New-FilterIndex($filters, $className) {
    if ($filters.Count -gt 10000) { throw 'Too many firewall filters to inspect safely.' }
    $index = [System.Collections.Generic.Dictionary[string,object]]::new([System.StringComparer]::Ordinal)
    foreach ($filter in $filters) {
        $id = [string]$filter.InstanceID
        if (!$id -or $filter.CimClass.CimClassName -cne $className -or $index.ContainsKey($id)) {
            throw 'Unexpected or duplicate firewall filter identity; refusing an ambiguous join.'
        }
        $index.Add($id, $filter)
    }
    return ,$index
}

function Get-RuleFilter($index, $rule) {
    $id = [string]$rule.InstanceID
    if (!$id -or $id -cne [string]$rule.Name -or !$index.ContainsKey($id)) {
        throw 'A firewall filter is missing or changed during inspection; check again.'
    }
    return $index[$id]
}

function Test-RelevantPort($filter) {
    if ([string]$filter.Protocol -notin @('TCP','UDP','6','17','Any','256')) { return $false }
    foreach ($value in $filter.LocalPort) {
        $number = 0
        if ([int]::TryParse([string]$value, [ref]$number)) {
            if ($number -eq $tcpPort -or $number -eq $udpPort) { return $true }
        } elseif ([string]$value -match '^(\d+)-(\d+)$') {
            if (($tcpPort -ge [int]$Matches[1] -and $tcpPort -le [int]$Matches[2]) -or
                ($udpPort -ge [int]$Matches[1] -and $udpPort -le [int]$Matches[2])) { return $true }
        } else { return $true }
    }
    return $false
}

function Read-Rules($store) {
    $rules = @(Get-NetFirewallRule -PolicyStore $store)
    if ($rules.Count -gt 10000) { throw 'Too many firewall rules to inspect safely.' }
    if ($store -eq 'PersistentStore') {
        $rules = @($rules | Where-Object { $_.Name -cin $checks.Name })
    } else {
        $rules = @($rules | Where-Object { [string]$_.Enabled -eq 'True' })
    }
    if (!$rules.Count) { return }
    $ports = New-FilterIndex @(Get-NetFirewallPortFilter -PolicyStore $store) 'MSFT_NetProtocolPortFilter'
    $rules = @(foreach ($rule in $rules) {
        $filter = Get-RuleFilter $ports $rule
        if ($store -eq 'PersistentStore' -or (Test-RelevantPort $filter)) { $rule }
    })
    if (!$rules.Count) { return }
    # Each collection is fetched once from the same policy store, not once per rule.
    $addresses = New-FilterIndex @(Get-NetFirewallAddressFilter -PolicyStore $store) 'MSFT_NetAddressFilter'
    $applications = New-FilterIndex @(Get-NetFirewallApplicationFilter -PolicyStore $store) 'MSFT_NetApplicationFilter'
    $services = New-FilterIndex @(Get-NetFirewallServiceFilter -PolicyStore $store) 'MSFT_NetServiceFilter'
    $interfaces = New-FilterIndex @(Get-NetFirewallInterfaceFilter -PolicyStore $store) 'MSFT_NetInterfaceFilter'
    $types = New-FilterIndex @(Get-NetFirewallInterfaceTypeFilter -PolicyStore $store) 'MSFT_NetInterfaceTypeFilter'
    $securities = New-FilterIndex @(Get-NetFirewallSecurityFilter -PolicyStore $store) 'MSFT_NetNetworkLayerSecurityFilter'
    $result = @(foreach ($r in $rules) {
        $p = Get-RuleFilter $ports $r
        $a = Get-RuleFilter $addresses $r
        $app = Get-RuleFilter $applications $r
        $service = Get-RuleFilter $services $r
        $iface = Get-RuleFilter $interfaces $r
        $type = Get-RuleFilter $types $r
        $security = Get-RuleFilter $securities $r
        [pscustomobject]@{
            name=[string]$r.Name; group=[string]$r.Group; description=[string]$r.Description
            enabled=([string]$r.Enabled -eq 'True'); direction=[string]$r.Direction
            action=[string]$r.Action; profile=[string]$r.Profile; protocol=[string]$p.Protocol
            localPort=@($p.LocalPort | ForEach-Object { [string]$_ }); remotePort=@($p.RemotePort | ForEach-Object { [string]$_ })
            localAddress=@($a.LocalAddress | ForEach-Object { [string]$_ }); remoteAddress=@($a.RemoteAddress | ForEach-Object { [string]$_ })
            program=[string]$app.Program; package=[string]$app.Package; service=[string]$service.Service
            interfaceAlias=@($iface.InterfaceAlias | ForEach-Object { [string]$_ }); interfaceType=[string]$type.InterfaceType
            localUser=[string]$security.LocalUser; remoteUser=[string]$security.RemoteUser; remoteMachine=[string]$security.RemoteMachine
            authentication=[string]$security.Authentication; encryption=[string]$security.Encryption; sourceType=[string]$r.PolicyStoreSourceType
        }
    })
    return $result
}

$checks = @(foreach ($direction in @('Inbound','Outbound')) {
    foreach ($protocol in @('tcp','udp')) {
        $port = if ($protocol -eq 'tcp') { $tcpPort } else { $udpPort }
        $short = if ($direction -eq 'Inbound') { 'in' } else { 'out' }
        [pscustomobject]@{ Name="fxins-fw-v1-$short-$protocol-$port"; Direction=$direction; Protocol=$protocol; Port=$port }
    }
})

function Is-Owned($r, $c) {
    $protocol = if ($c.Protocol -eq 'tcp') { '6' } else { '17' }
    return ($r.name -ceq $c.Name -and $r.group -ceq $group -and $r.description -ceq $description -and
        $r.sourceType -eq 'Local' -and $r.action -eq 'Allow' -and $r.direction -eq $c.Direction -and
        $r.protocol -in @($c.Protocol,$protocol) -and $r.profile -in @('Any','Domain, Private, Public') -and
        $r.localPort.Count -eq 1 -and $r.localPort[0] -eq [string]$c.Port -and
        ($r.remotePort -join ',') -eq 'Any' -and ($r.localAddress -join ',') -eq 'Any' -and ($r.remoteAddress -join ',') -eq 'Any' -and
        $r.program -in @('','Any') -and $r.package -in @('','Any') -and $r.service -in @('','Any') -and
        ($r.interfaceAlias -join ',') -eq 'Any' -and $r.interfaceType -eq 'Any' -and
        $r.localUser -in @('','Any') -and $r.remoteUser -in @('','Any') -and $r.remoteMachine -in @('','Any') -and
        $r.authentication -eq 'NotRequired' -and $r.encryption -eq 'NotRequired')
}

try {
if ($operation -ne 'inspect') {
    foreach ($index in $selected) {
        $c = $checks[$index]
        $existing = @(Read-Rules 'PersistentStore' | Where-Object { $_.name -ceq $c.Name })
        if ($existing.Count -gt 1 -or ($existing.Count -eq 1 -and !(Is-Owned $existing[0] $c))) {
            throw 'A reserved firewall name has different settings; refusing to change it.'
        }
        if ($operation -eq 'remove' -and $existing.Count -eq 1) {
            Get-NetFirewallRule -PolicyStore PersistentStore -Name $c.Name | Remove-NetFirewallRule
        } elseif ($operation -eq 'configure' -and $existing.Count -eq 0) {
            New-NetFirewallRule -PolicyStore PersistentStore -Name $c.Name -DisplayName "FiveM $($c.Protocol) $($c.Direction) $($c.Port)" `
                -Description $description -Group $group -Direction $c.Direction -Protocol $c.Protocol -LocalPort $c.Port `
                -RemotePort Any -LocalAddress Any -RemoteAddress Any -Profile Any -Enabled True -Action Allow | Out-Null
        } elseif ($operation -eq 'configure' -and $existing.Count -eq 1) {
            Get-NetFirewallRule -PolicyStore PersistentStore -Name $c.Name | Enable-NetFirewallRule
        }
    }
}
$profiles = @(Get-NetFirewallProfile -PolicyStore ActiveStore | ForEach-Object {
    [pscustomobject]@{ enabled=([string]$_.Enabled -eq 'True'); defaultInbound=[string]$_.DefaultInboundAction
        defaultOutbound=[string]$_.DefaultOutboundAction; allowInbound=[string]$_.AllowInboundRules; allowLocal=[string]$_.AllowLocalFirewallRules }
})
[pscustomobject]@{ profiles=$profiles; rules=@(Read-Rules 'ActiveStore'); owned=@(Read-Rules 'PersistentStore') } | ConvertTo-Json -Depth 8 -Compress
} catch {
    if ($operation -eq 'inspect' -and ($_.CategoryInfo.Category -eq [System.Management.Automation.ErrorCategory]::PermissionDenied -or
        $_.Exception -is [System.UnauthorizedAccessException])) {
        [pscustomobject]@{ requiresElevation=$true; profiles=@(); rules=@(); owned=@() } | ConvertTo-Json -Compress
    } else { throw }
}
