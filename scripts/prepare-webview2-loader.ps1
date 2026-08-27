param(
    [Parameter(Mandatory = $false)]
    [string]$Destination
)

$ErrorActionPreference = 'Stop'

if ([string]::IsNullOrWhiteSpace($Destination)) {
    $Destination = Join-Path $PSScriptRoot '..\src-tauri\resources\windows\x64\WebView2Loader.dll'
}

# Use the framework API because stripped-down build environments may not load
# Microsoft.PowerShell.Utility, which provides Get-FileHash.
function Get-Sha256 {
    param([Parameter(Mandatory = $true)][string]$Path)

    $stream = [System.IO.File]::OpenRead($Path)
    try {
        $sha256 = [System.Security.Cryptography.SHA256]::Create()
        try {
            return ([System.BitConverter]::ToString($sha256.ComputeHash($stream))).Replace('-', '')
        }
        finally {
            $sha256.Dispose()
        }
    }
    finally {
        $stream.Dispose()
    }
}

# Keep the SDK aligned with webview2-com-sys 0.38.2 instead of silently tracking
# the newest WebView2 package, whose native ABI has not been validated here.
$sdkVersion = '1.0.3650.58'
$packageSha256 = '911A472128C82AC8BAA0C486C23342CC9DD6E7DC50D754E676726642CA065C60'
$loaderSha256 = '8427B1FC58EC707813E5C0A51EB5D69397BB333250A7B891BE4D3B123F1E0F1C'
$packageUri = "https://api.nuget.org/v3-flatcontainer/microsoft.web.webview2/$sdkVersion/microsoft.web.webview2.$sdkVersion.nupkg"
$loaderEntry = 'runtimes/win-x64/native/WebView2Loader.dll'

$resolvedDestination = [System.IO.Path]::GetFullPath($Destination)
if (Test-Path -LiteralPath $resolvedDestination -PathType Leaf) {
    $existingHash = Get-Sha256 -Path $resolvedDestination
    if ($existingHash -eq $loaderSha256) {
        Write-Host "WebView2Loader.dll $sdkVersion is already available."
        exit 0
    }

    Write-Warning 'The existing WebView2Loader.dll failed validation and will be replaced.'
}

$destinationDirectory = Split-Path -Parent $resolvedDestination
New-Item -ItemType Directory -Path $destinationDirectory -Force | Out-Null

$downloadPath = Join-Path ([System.IO.Path]::GetTempPath()) ("webview2-sdk-{0}.nupkg" -f [guid]::NewGuid().ToString('N'))
$stagedLoader = "$resolvedDestination.download"

try {
    # Windows PowerShell 5.1 may otherwise negotiate an obsolete TLS version.
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    Write-Host "Downloading Microsoft WebView2 SDK $sdkVersion from NuGet..."
    Invoke-WebRequest -Uri $packageUri -OutFile $downloadPath -UseBasicParsing

    $downloadHash = Get-Sha256 -Path $downloadPath
    if ($downloadHash -ne $packageSha256) {
        throw "WebView2 SDK checksum mismatch. Expected $packageSha256, received $downloadHash."
    }

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [System.IO.Compression.ZipFile]::OpenRead($downloadPath)
    try {
        $entry = $archive.Entries | Where-Object { $_.FullName -eq $loaderEntry } | Select-Object -First 1
        if ($null -eq $entry) {
            throw "The official SDK package does not contain $loaderEntry."
        }

        $inputStream = $entry.Open()
        try {
            $outputStream = [System.IO.File]::Create($stagedLoader)
            try {
                $inputStream.CopyTo($outputStream)
            }
            finally {
                $outputStream.Dispose()
            }
        }
        finally {
            $inputStream.Dispose()
        }
    }
    finally {
        $archive.Dispose()
    }

    $extractedHash = Get-Sha256 -Path $stagedLoader
    if ($extractedHash -ne $loaderSha256) {
        throw "Extracted WebView2Loader.dll checksum mismatch. Expected $loaderSha256, received $extractedHash."
    }

    Move-Item -LiteralPath $stagedLoader -Destination $resolvedDestination -Force
    Write-Host "Prepared WebView2Loader.dll $sdkVersion at $resolvedDestination."
}
finally {
    Remove-Item -LiteralPath $downloadPath -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $stagedLoader -Force -ErrorAction SilentlyContinue
}
