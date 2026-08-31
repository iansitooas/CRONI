[CmdletBinding()]
param(
    [switch]$RuntimeOnly
)

$ErrorActionPreference = "Stop"

if (-not (Get-Command winget.exe -ErrorAction SilentlyContinue)) {
    throw "No se encontro winget. Instala 'Instalador de aplicaciones' desde Microsoft Store y vuelve a ejecutar este script."
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

Install-WingetPackage -Id "Microsoft.EdgeWebView2Runtime"

if (-not $RuntimeOnly) {
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
