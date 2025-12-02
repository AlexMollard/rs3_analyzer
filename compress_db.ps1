# Compress the database for version control
$dbFile = "rs3_market.db"
$compressedFile = "rs3_market.db.gz"

if (Test-Path $dbFile) {
    Write-Host "Compressing $dbFile..."
    
    $input = [System.IO.File]::OpenRead($dbFile)
    $output = [System.IO.File]::Create($compressedFile)
    $gzipStream = New-Object System.IO.Compression.GZipStream($output, [System.IO.Compression.CompressionMode]::Compress)
    
    $input.CopyTo($gzipStream)
    
    $gzipStream.Close()
    $output.Close()
    $input.Close()
    
    $originalSize = (Get-Item $dbFile).Length / 1MB
    $compressedSize = (Get-Item $compressedFile).Length / 1MB
    $ratio = [math]::Round(($compressedSize / $originalSize) * 100, 2)
    
    Write-Host "Compression complete!"
    Write-Host "Original size: $([math]::Round($originalSize, 2)) MB"
    Write-Host "Compressed size: $([math]::Round($compressedSize, 2)) MB"
    Write-Host "Compression ratio: $ratio%"
} else {
    Write-Host "Error: $dbFile not found!"
    exit 1
}
