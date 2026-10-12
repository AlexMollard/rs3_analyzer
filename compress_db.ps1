# Compress the database into a snapshot that can be shared outside GitHub
$ErrorActionPreference = 'Stop'

$dbFile = Join-Path $PSScriptRoot "rs3_market.db"
$compressedFile = Join-Path $PSScriptRoot "rs3_market.db.gz"
$tmpFile = "$compressedFile.tmp"

if (-not (Test-Path -LiteralPath $dbFile)) {
    Write-Host "Error: $dbFile not found!"
    exit 1
}

Write-Host "Compressing $dbFile..."

$inStream = $null
$output = $null
$gzipStream = $null
try {
    # FileShare.ReadWrite so a database that is open elsewhere can still be read
    $inStream = New-Object System.IO.FileStream($dbFile, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]::ReadWrite)
    $output = [System.IO.File]::Create($tmpFile)
    $gzipStream = New-Object System.IO.Compression.GZipStream($output, [System.IO.Compression.CompressionLevel]::Optimal)

    $inStream.CopyTo($gzipStream)

    # Close the gzip stream first so the trailer is flushed before the move
    $gzipStream.Close()
    $gzipStream = $null
    $output.Close()
    $output = $null
    $inStream.Close()
    $inStream = $null

    Move-Item -LiteralPath $tmpFile -Destination $compressedFile -Force
}
catch {
    Write-Host "Error: compression failed: $($_.Exception.Message)"
    exit 1
}
finally {
    if ($gzipStream) { $gzipStream.Dispose() }
    if ($output) { $output.Dispose() }
    if ($inStream) { $inStream.Dispose() }
    if (Test-Path -LiteralPath $tmpFile) { Remove-Item -LiteralPath $tmpFile -Force }
}

$originalSize = (Get-Item -LiteralPath $dbFile).Length / 1MB
$compressedSize = (Get-Item -LiteralPath $compressedFile).Length / 1MB
$ratio = [math]::Round(($compressedSize / $originalSize) * 100, 2)

Write-Host "Compression complete!"
Write-Host "Original size: $([math]::Round($originalSize, 2)) MB"
Write-Host "Compressed size: $([math]::Round($compressedSize, 2)) MB"
Write-Host "Compression ratio: $ratio%"
Write-Host "SHA256: $((Get-FileHash -LiteralPath $compressedFile -Algorithm SHA256).Hash)"
