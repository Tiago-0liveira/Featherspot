$ErrorActionPreference = 'Stop'

$repository = 'Tiago-0liveira/lspotify'
$releaseEndpoint = "https://api.github.com/repos/$repository/releases/latest"
$temporaryDirectory = Join-Path ([System.IO.Path]::GetTempPath()) ("lspotify-install-" + [guid]::NewGuid().ToString('N'))

try {
    if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT -or -not [Environment]::Is64BitOperatingSystem) {
        throw 'lspotify currently requires 64-bit Windows.'
    }
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    $release = Invoke-RestMethod -Uri $releaseEndpoint -Headers @{ 'User-Agent' = 'lspotify installer' }
    if ($release.tag_name -notmatch '^v\d+\.\d+\.\d+$') {
        throw "The latest release has an unexpected version: $($release.tag_name)"
    }

    $msiName = "lspotify-$($release.tag_name)-windows-x64-unsigned.msi"
    $msiAsset = @($release.assets | Where-Object { $_.name -eq $msiName })
    $checksumsAsset = @($release.assets | Where-Object { $_.name -eq 'SHA256SUMS.txt' })
    if ($msiAsset.Count -ne 1 -or $checksumsAsset.Count -ne 1) {
        throw "Release $($release.tag_name) is missing its MSI or checksum file."
    }

    $running = @(Get-Process -Name 'lspotify', 'lspotify-player-host' -ErrorAction SilentlyContinue)
    if ($running.Count -gt 0) {
        throw 'Close lspotify before installing or upgrading it.'
    }

    New-Item -ItemType Directory -Path $temporaryDirectory | Out-Null
    $msiPath = Join-Path $temporaryDirectory $msiName
    $checksumsPath = Join-Path $temporaryDirectory 'SHA256SUMS.txt'
    Invoke-WebRequest -Uri $msiAsset[0].browser_download_url -OutFile $msiPath -UseBasicParsing
    Invoke-WebRequest -Uri $checksumsAsset[0].browser_download_url -OutFile $checksumsPath -UseBasicParsing

    $expectedHash = $null
    foreach ($line in Get-Content $checksumsPath) {
        if ($line -match '^([0-9a-fA-F]{64})\s+\*?(.+)$' -and $Matches[2] -eq $msiName) {
            $expectedHash = $Matches[1]
            break
        }
    }
    if (-not $expectedHash) {
        throw "SHA256SUMS.txt has no checksum for $msiName."
    }
    $actualHash = (Get-FileHash -Path $msiPath -Algorithm SHA256).Hash
    if ($actualHash -ine $expectedHash) {
        throw "Checksum verification failed for $msiName."
    }

    Write-Host "Installing lspotify $($release.tag_name)..."
    $installer = Start-Process -FilePath 'msiexec.exe' -ArgumentList @('/i', "`"$msiPath`"", '/qn', '/norestart') -Wait -PassThru
    if ($installer.ExitCode -notin @(0, 3010)) {
        throw "Windows Installer failed with exit code $($installer.ExitCode)."
    }
    Write-Host "lspotify $($release.tag_name) is installed. Open a new terminal and run lspotify."
    if ($installer.ExitCode -eq 3010) {
        Write-Warning 'Windows requested a restart to finish installation.'
    }
} catch {
    [Console]::Error.WriteLine("lspotify installation failed: $_")
    exit 1
} finally {
    if (Test-Path -LiteralPath $temporaryDirectory) {
        Remove-Item -LiteralPath $temporaryDirectory -Recurse -Force
    }
}
