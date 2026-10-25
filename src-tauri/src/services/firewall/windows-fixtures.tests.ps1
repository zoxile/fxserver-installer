$ErrorActionPreference = 'Stop'
$tokens = $null
$parseErrors = $null
$ast = [System.Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'windows.ps1'), [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count) { throw ($parseErrors | Out-String) }
foreach ($name in @('Is-Owned','New-FilterIndex','Get-RuleFilter','Test-RelevantPort')) {
    $definition = $ast.Find({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name }, $false)
    if (!$definition) { throw "Pure function $name not found." }
    # Load only pure functions, never the firewall command or its imports.
    . ([scriptblock]::Create($definition.Extent.Text))
}
$group = 'FXServer Installer - FiveM v1'
$description = 'Managed by FXServer Installer. FiveM host port rule v1.'
$rule = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'fixtures/windows_rule.json') -Raw | ConvertFrom-Json
$check = [pscustomobject]@{ Name='fxins-fw-v1-in-tcp-30120'; Direction='Inbound'; Protocol='tcp'; Port=30120 }
if (Is-Owned $rule $check) { throw 'External rule incorrectly identified as owned.' }
$rule.name = $check.Name; $rule.group = $group; $rule.description = $description
if (!(Is-Owned $rule $check)) { throw 'Exact ownership marker was not recognized.' }
foreach ($property in @('group','description','action','profile','remoteAddress','localPort','localUser','sourceType','interfaceAlias','program','service')) {
    $original = $rule.$property
    $rule.$property = 'unexpected'
    if (Is-Owned $rule $check) { throw "Modified $property incorrectly identified as owned." }
    $rule.$property = $original
}
$rule.direction = 'Outbound'; $check.Direction = 'Outbound'; $rule.name = 'fxins-fw-v1-out-tcp-30120'; $check.Name=$rule.name
if (!(Is-Owned $rule $check)) { throw 'Outbound rule must match the local port, not a destination port.' }

$filter = [pscustomobject]@{ InstanceID='fixture-rule'; CimClass=[pscustomobject]@{CimClassName='MSFT_NetProtocolPortFilter'}; Protocol='TCP'; LocalPort=@('30120') }
$identity = [pscustomobject]@{ Name='fixture-rule'; InstanceID='fixture-rule' }
$index = New-FilterIndex @($filter) 'MSFT_NetProtocolPortFilter'
if ((Get-RuleFilter $index $identity) -ne $filter) { throw 'The exact filter join failed.' }
foreach ($case in @('duplicate','class','missing','mismatched','case')) {
    $failed=$false
    try {
        switch ($case) {
            duplicate { $null=New-FilterIndex @($filter,$filter) 'MSFT_NetProtocolPortFilter' }
            class { $null=New-FilterIndex @($filter) 'MSFT_NetServiceFilter' }
            missing { $null=Get-RuleFilter $index ([pscustomobject]@{Name='absent';InstanceID='absent'}) }
            mismatched { $null=Get-RuleFilter $index ([pscustomobject]@{Name='other';InstanceID='fixture-rule'}) }
            case { $null=Get-RuleFilter $index ([pscustomobject]@{Name='FIXTURE-RULE';InstanceID='FIXTURE-RULE'}) }
        }
    } catch { $failed=$true }
    if (!$failed) { throw "Unsafe filter join accepted: $case" }
}
$tcpPort=30120; $udpPort=30121
if (!(Test-RelevantPort $filter)) { throw 'Requested port filtered out.' }
$filter.LocalPort=@('80'); if (Test-RelevantPort $filter) { throw 'Unrelated port was retained.' }
$filter.LocalPort=@('30100-30200'); if (!(Test-RelevantPort $filter)) { throw 'Matching range filtered out.' }
$filter.LocalPort=@('RPC'); if (!(Test-RelevantPort $filter)) { throw 'Dynamic ports must be checked conservatively.' }
'Windows firewall parser and ownership fixtures passed; no firewall cmdlets were invoked.'
