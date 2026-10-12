# Decompress a shared database snapshot (rs3_market.db.gz) into rs3_market.db
param(
    [switch]$Force  # overwrite an existing rs3_market.db
)

$ErrorActionPreference = 'Stop'

$compressedFile = Join-Path $PSScriptRoot "rs3_market.db.gz"
$dbFile = Join-Path $PSScriptRoot "rs3_market.db"
$tmpFile = "$dbFile.tmp"

if (-not (Test-Path -LiteralPath $compressedFile)) {
    Write-Host "Error: $compressedFile not found!"
    Write-Host "The snapshot is not stored in git. Get it from wherever it was shared, or rebuild with backfill_history.py."
    exit 1
}

if ((Test-Path -LiteralPath $dbFile) -and -not $Force) {
    Write-Host "Error: $dbFile already exists. Re-run with -Force to overwrite it."
    exit 1
}

Write-Host "Decompressing $compressedFile..."

$inStream = $null
$output = $null
$gzipStream = $null
try {
    $inStream = [System.IO.File]::OpenRead($compressedFile)
    $output = [System.IO.File]::Create($tmpFile)
    $gzipStream = New-Object System.IO.Compression.GZipStream($inStream, [System.IO.Compression.CompressionMode]::Decompress)

    $gzipStream.CopyTo($output)

    $gzipStream.Close()
    $gzipStream = $null
    $output.Close()
    $output = $null
    $inStream.Close()
    $inStream = $null

    Move-Item -LiteralPath $tmpFile -Destination $dbFile -Force
}
catch {
    Write-Host "Error: decompression failed: $($_.Exception.Message)"
    exit 1
}
finally {
    if ($gzipStream) { $gzipStream.Dispose() }
    if ($output) { $output.Dispose() }
    if ($inStream) { $inStream.Dispose() }
    if (Test-Path -LiteralPath $tmpFile) { Remove-Item -LiteralPath $tmpFile -Force }
}

$size = (Get-Item -LiteralPath $dbFile).Length / 1MB
Write-Host "Decompression complete!"
Write-Host "Database size: $([math]::Round($size, 2)) MB"
Write-Host "SHA256: $((Get-FileHash -LiteralPath $dbFile -Algorithm SHA256).Hash)"
Write-Host "You can now run the application with: pnpm tauri dev"
