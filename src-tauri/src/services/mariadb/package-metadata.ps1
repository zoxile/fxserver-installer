try {
    $releases = @(Get-MariaDBReleases $series)
    if ($requested -match '^\d+\.\d+\.\d+$') {
        $release = $releases | Where-Object { $_.release_id -ceq $requested } | Select-Object -First 1
    } else {
        $release = $releases | Select-Object -First 1
    }
    if (-not $release) { throw 'The selected stable Windows release is unavailable. No substitute was selected.' }
    $file = Get-MariaDBMsi $release
    [pscustomobject]@{
        version = $release.release_id
        file_name = $file.file_name
        sha256 = $file.checksum.sha256sum
    } | ConvertTo-Json -Compress
} catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
}
