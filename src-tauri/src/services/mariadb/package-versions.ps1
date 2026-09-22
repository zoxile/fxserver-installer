$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

function Get-MariaDBMetadata([string]$path) {
    if ($path -notmatch '^(?:\d{1,4}\.\d{1,4}/)?$') { throw 'Invalid metadata path.' }
    $request = [Net.HttpWebRequest]::Create("https://downloads.mariadb.org/rest-api/mariadb/$path")
    $request.AllowAutoRedirect = $false
    $request.Timeout = 20000
    $request.ReadWriteTimeout = 20000
    $response = $null
    $stream = $null
    $bytes = New-Object IO.MemoryStream
    $clock = [Diagnostics.Stopwatch]::StartNew()
    try {
        $response = $request.GetResponse()
        if ([int]$response.StatusCode -ne 200 -or $response.ContentLength -gt 4194304) { throw 'Unexpected or oversized MariaDB metadata response.' }
        $stream = $response.GetResponseStream()
        $buffer = New-Object byte[] 8192
        while (($count = $stream.Read($buffer, 0, $buffer.Length)) -gt 0) {
            if ($bytes.Length + $count -gt 4194304 -or $clock.Elapsed.TotalSeconds -gt 25) { throw 'MariaDB metadata exceeded its size or time limit.' }
            $bytes.Write($buffer, 0, $count)
        }
        [Text.Encoding]::UTF8.GetString($bytes.ToArray()) | ConvertFrom-Json
    } finally {
        if ($stream) { $stream.Dispose() }
        if ($response) { $response.Dispose() }
        $bytes.Dispose()
    }
}

function Get-MariaDBSeries([bool]$includeRolling = $false) {
    $index = Get-MariaDBMetadata ''
    if (@($index.major_releases).Count -gt 100) { throw 'Too many MariaDB series in metadata.' }
    foreach ($item in $index.major_releases) {
        if ($item.release_id -notmatch '^\d{1,4}\.\d{1,4}$' -or $item.release_status -cne 'Stable') { continue }
        if ($includeRolling -and $item.release_support_type -ceq 'Rolling' -and -not $item.release_eol_date) {
            # Only currently listed stable rolling series qualify; never guess a replacement.
            [pscustomobject]@{ series = $item.release_id; support = 'Rolling'; eol = $null }
            continue
        }
        if ($item.release_support_type -cne 'Long Term Support' -and -not ($includeRolling -and $item.release_support_type -ceq 'Rolling')) { continue }
        $eol = $item.release_eol_date
        # Community dates from https://mariadb.org/about/; the API can include enterprise dates.
        if ($item.release_id -eq '11.8' -and $eol -gt '2028-06-04') { $eol = '2028-06-04' }
        if ($eol -notmatch '^\d{4}-\d{2}-\d{2}$' -or [datetime]::ParseExact($eol, 'yyyy-MM-dd', [Globalization.CultureInfo]::InvariantCulture) -le [datetime]::UtcNow.Date) { continue }
        [pscustomobject]@{ series = $item.release_id; support = $item.release_support_type; eol = $eol }
    }
}

function Get-MariaDBMsi($release) {
    @($release.files | Where-Object {
        $_.os -ceq 'Windows' -and $_.cpu -ceq 'x86_64' -and
        $_.file_name -ceq "mariadb-$($release.release_id)-winx64.msi" -and
        $_.checksum.sha256sum -match '^[a-fA-F0-9]{64}$'
    }) | Select-Object -First 1
}

function Get-MariaDBReleases([string]$series) {
    if ($series -notmatch '^\d{1,4}\.\d{1,4}$') { throw 'Invalid MariaDB series.' }
    if (-not (Get-MariaDBSeries ([bool]$allowRolling) | Where-Object { $_.series -ceq $series })) { throw 'This series is not currently supported under the selected Community release policy. Plan a backed-up migration; no other series was selected.' }
    $metadata = Get-MariaDBMetadata "$series/"
    $releases = @($metadata.releases.PSObject.Properties.Value)
    if ($releases.Count -gt 256) { throw 'Too many MariaDB releases in metadata.' }
    $releases | Where-Object {
        $_.release_id -match '^\d{1,4}\.\d{1,4}\.\d{1,4}$' -and
        $_.release_id.StartsWith("$series.", [StringComparison]::Ordinal) -and
        $_.release_name -ceq "MariaDB Server $($_.release_id)" -and
        $_.date_of_release -match '^\d{4}-\d{2}-\d{2}$' -and
        $_.date_of_release -le [datetime]::UtcNow.ToString('yyyy-MM-dd') -and
        (Get-MariaDBMsi $_)
    } | Sort-Object { [version]$_.release_id } -Descending
}

if (-not $resolveOnly) {
    try {
        if ($series) {
            $result = @(Get-MariaDBReleases $series | ForEach-Object { [pscustomobject]@{ version = $_.release_id; date = $_.date_of_release } })
        } else {
            $result = @(Get-MariaDBSeries | Sort-Object { [version]$_.series } -Descending)
        }
        ConvertTo-Json -InputObject $result -Compress
    } catch {
        [Console]::Error.WriteLine($_.Exception.Message)
        exit 1
    }
}
