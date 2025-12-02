# Decompress the database after cloning the repository
$compressedFile = "rs3_market.db.gz"
$dbFile = "rs3_market.db"

if (Test-Path $compressedFile) {
    Write-Host "Decompressing $compressedFile..."
    
    $input = [System.IO.File]::OpenRead($compressedFile)
    $output = [System.IO.File]::Create($dbFile)
    $gzipStream = New-Object System.IO.Compression.GZipStream($input, [System.IO.Compression.CompressionMode]::Decompress)
    
    $gzipStream.CopyTo($output)
    
    $gzipStream.Close()
    $input.Close()
    $output.Close()
    
    $size = (Get-Item $dbFile).Length / 1MB
    Write-Host "Decompression complete!"
    Write-Host "Database size: $([math]::Round($size, 2)) MB"
    Write-Host "You can now run the application with: cargo run"
} else {
    Write-Host "Error: $compressedFile not found!"
    Write-Host "The database file may need to be downloaded separately."
    exit 1
}
