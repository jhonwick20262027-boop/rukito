<#
.SYNOPSIS
    Instalador de rdd para Windows.

.DESCRIPTION
    Descarga el binario de la ultima release (o de la indicada en -Version),
    verifica su SHA256 y lo instala sin privilegios de administrador.

.EXAMPLE
    powershell -NoProfile -Command "iex (irm 'https://raw.githubusercontent.com/jhonwick20262027-boop/rukito/main/docs/install.ps1')"

.EXAMPLE
    $env:RDD_VERSION = 'v0.1.0'; .\install.ps1
#>
[CmdletBinding()]
param(
    [string]$Version = $env:RDD_VERSION,
    [string]$InstallDir = $(if ($env:RDD_INSTALL_DIR) { $env:RDD_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'rdd\bin' })
)

$ErrorActionPreference = 'Stop'
$repo = 'jhonwick20262027-boop/rukito'

$arch = $env:PROCESSOR_ARCHITECTURE
try {
    $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
} catch {
    # Windows PowerShell 5.1 siempre trae PROCESSOR_ARCHITECTURE.
}
switch ($arch) {
    'X64' { $target = 'x86_64-pc-windows-msvc' }
    'Arm64' { throw 'todavia no hay binario para Windows ARM64; usa cargo install rdd-cli' }
    default { throw "arquitectura no soportada: $arch" }
}

if (-not $Version) {
    try {
        $latest = Invoke-RestMethod -UseBasicParsing -Uri "https://api.github.com/repos/$repo/releases/latest"
        $Version = $latest.tag_name
    } catch {
        $status = $null
        if ($_.Exception.Response) { $status = [int]$_.Exception.Response.StatusCode }
        if ($status -eq 404) {
            throw "todavia no hay una release publicada de rdd; usa -Version o 'cargo install rdd-cli' mientras tanto"
        }
        throw "no se pudo resolver la ultima version ($($_.Exception.Message)); fija -Version o usa 'cargo install rdd-cli'"
    }
}
if (-not $Version) { throw 'no se pudo resolver la ultima version' }

$version = $Version.TrimStart('v')
$asset = "rdd-$version-$target.zip"
$base = "https://github.com/$repo/releases/download/$Version"
$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("rdd-" + [guid]::NewGuid().ToString('N'))

New-Item -ItemType Directory -Path $tmp -Force | Out-Null
try {
    Write-Host "Descargando $asset..."
    $zip = Join-Path $tmp $asset
    Invoke-WebRequest -UseBasicParsing -Uri "$base/$asset" -OutFile $zip

    $sums = (Invoke-WebRequest -UseBasicParsing -Uri "$base/SHA256SUMS.txt").Content -split "`n"
    $pattern = "\s\*?" + [regex]::Escape($asset) + "$"
    $line = $sums | Where-Object { $_ -match $pattern } | Select-Object -First 1
    if (-not $line) { throw "el checksum de $asset no esta en SHA256SUMS.txt" }
    $expected = ($line -split '\s+')[0]
    $actual = (Get-FileHash -Algorithm SHA256 -Path $zip).Hash
    if ($actual -ne $expected) { throw "checksum incorrecto para $asset" }

    Expand-Archive -Path $zip -DestinationPath $tmp -Force
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    $exe = Join-Path $InstallDir 'rdd.exe'
    Copy-Item (Join-Path $tmp "rdd-$version-$target/rdd.exe") $exe -Force

    & $exe --version

    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $parts = @()
    if ($userPath) { $parts = $userPath -split ';' | Where-Object { $_ } }
    if ($parts -notcontains $InstallDir) {
        [Environment]::SetEnvironmentVariable('Path', (($parts + $InstallDir) -join ';'), 'User')
        Write-Host "Anadido $InstallDir al PATH del usuario; abre una terminal nueva."
    }
    $env:Path = "$env:Path;$InstallDir"
} finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
