# installs the nana binary from the latest GitHub release.
# usage: irm https://raw.githubusercontent.com/mitige/nana/main/install.ps1 | iex
# the binary goes to %LOCALAPPDATA%\nana\bin unless NANA_INSTALL_DIR says otherwise.

$ErrorActionPreference = "Stop"

$asset = "nana-windows-x86_64.zip"
$url = "https://github.com/mitige/nana/releases/latest/download/$asset"
$dir = if ($env:NANA_INSTALL_DIR) { $env:NANA_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "nana\bin" }

$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("nana-" + [System.Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $tmp | Out-Null

try {
    Write-Host "nana: downloading $asset"
    $zip = Join-Path $tmp $asset
    Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing
    Expand-Archive -Path $zip -DestinationPath $tmp -Force

    New-Item -ItemType Directory -Path $dir -Force | Out-Null
    Copy-Item -Path (Join-Path $tmp "nana.exe") -Destination (Join-Path $dir "nana.exe") -Force

    $version = & (Join-Path $dir "nana.exe") --version
    Write-Host "nana: installed $version to $dir\nana.exe"

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if (-not (($userPath -split ";") -contains $dir)) {
        [Environment]::SetEnvironmentVariable("Path", (($userPath, $dir) -join ";").TrimStart(";"), "User")
        Write-Host "nana: $dir was added to your user PATH; open a new terminal"
    }
}
finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
