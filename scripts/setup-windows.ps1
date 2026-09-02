[CmdletBinding()]
param(
    [switch]$RuntimeOnly
)

$ErrorActionPreference = "Stop"

$currentIdentity = [Security.Principal.WindowsIdentity]::GetCurrent()
$currentPrincipal = [Security.Principal.WindowsPrincipal]::new($currentIdentity)
$isAdministrator = $currentPrincipal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdministrator) {
    Write-Host "[CRONI] Windows pedira permiso para actualizar los componentes oficiales."
    $elevationArguments = @(
        "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", ('"{0}"' -f $PSCommandPath)
    )
    if ($RuntimeOnly) {
        $elevationArguments += "-RuntimeOnly"
    }
    $elevatedProcess = Start-Process `
        -FilePath "powershell.exe" `
        -ArgumentList $elevationArguments `
        -Verb RunAs `
        -Wait `
        -PassThru
    exit $elevatedProcess.ExitCode
}

function Install-LatestWebView2 {
    $installerPath = Join-Path ([System.IO.Path]::GetTempPath()) "CRONI-WebView2Setup.exe"
    Write-Host "[CRONI] Descargando la version actual de WebView2 desde Microsoft..."
    try {
        Invoke-WebRequest `
            -Uri "https://go.microsoft.com/fwlink/p/?LinkId=2124703" `
            -OutFile $installerPath `
            -UseBasicParsing
        $installer = Start-Process `
            -FilePath $installerPath `
            -ArgumentList "/silent", "/install" `
            -WindowStyle Hidden `
            -Wait `
            -PassThru
        $successfulExitCodes = @(0, -2147219416, -2147219187)
        if ($installer.ExitCode -notin $successfulExitCodes) {
            throw "El instalador oficial de WebView2 termino con el codigo $($installer.ExitCode)."
        }
        Write-Host "[OK] WebView2 esta instalado y el actualizador de Microsoft queda habilitado."
    } finally {
        Remove-Item -LiteralPath $installerPath -Force -ErrorAction SilentlyContinue
    }
}

function Install-WingetPackage {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Id,
        [string]$Override = ""
    )

    & winget.exe list --id $Id --exact --source winget --accept-source-agreements *> $null
    if ($LASTEXITCODE -eq 0) {
        Write-Host "[OK] $Id ya esta instalado."
        return
    }

    Write-Host "[CRONI] Instalando $Id..."
    $arguments = @(
        "install", "--id", $Id, "--exact", "--source", "winget",
        "--accept-package-agreements", "--accept-source-agreements"
    )
    if ($Override) {
        $arguments += @("--override", $Override)
    }
    & winget.exe @arguments
    if ($LASTEXITCODE -ne 0) {
        throw "No se pudo instalar $Id (codigo $LASTEXITCODE)."
    }
}

Install-LatestWebView2

if (-not $RuntimeOnly) {
    if (-not (Get-Command winget.exe -ErrorAction SilentlyContinue)) {
        throw "No se encontro winget. Instala 'Instalador de aplicaciones' desde Microsoft Store y vuelve a ejecutar este script."
    }
    Install-WingetPackage -Id "Rustlang.Rustup"
    Install-WingetPackage `
        -Id "Microsoft.VisualStudio.2022.BuildTools" `
        -Override "--wait --passive --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"

    Write-Host ""
    Write-Host "Recursos de desarrollo preparados. Cierra y abre PowerShell antes de ejecutar cargo."
    Write-Host "Después: cargo test; cargo run --release"
} else {
    Write-Host "WebView2 esta preparado. Ya puedes abrir CRONI.exe."
}
