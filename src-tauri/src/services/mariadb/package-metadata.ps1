$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$api = 'https://downloads.mariadb.org/rest-api/mariadb'

try {
    $index = Invoke-RestMethod -Uri "$api/" -TimeoutSec 30
    $series = $index.major_releases |
        Where-Object { $_.release_status -eq 'Stable' -and $_.release_id -match '^\d+\.\d+$' } |
        Sort-Object { [version]$_.release_id } -Descending |
        Select-Object -First 1
    if (-not $series) { throw 'No stable MariaDB release was found.' }

    $metadata = Invoke-RestMethod -Uri "$api/$($series.release_id)/latest/" -TimeoutSec 30
    $release = $metadata.releases.PSObject.Properties.Value | Select-Object -First 1
    $file = $release.files |
        Where-Object { $_.os -eq 'Windows' -and $_.cpu -eq 'x86_64' -and $_.file_name -match '-winx64\.msi$' } |
        Select-Object -First 1
    if (-not $file) { throw 'No Windows x64 MSI was published for this MariaDB release.' }

    [pscustomobject]@{
        version = $release.release_id
        file_name = $file.file_name
        sha256 = $file.checksum.sha256sum
    } | ConvertTo-Json -Compress
} catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
}
