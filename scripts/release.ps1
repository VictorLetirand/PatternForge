# Vérifie le dépôt, compile en release et assemble dist\PatternForge-<version>-windows-x64.zip
# Usage : powershell -ExecutionPolicy Bypass -File scripts\release.ps1
$ErrorActionPreference = "Stop"
$racine = Split-Path -Parent $PSScriptRoot
Set-Location $racine

function Etape($nom, [scriptblock]$commande) {
    Write-Host "== $nom"
    & $commande
    if ($LASTEXITCODE -ne 0) { throw "Échec : $nom" }
}

Etape "format" { cargo fmt --all -- --check }
# Même version de clippy que la CI (le stable local peut être plus ancien et laisser passer).
Etape "clippy" { cargo +1.99.1 clippy --workspace --all-targets -- -D warnings }
Etape "tests" { cargo test --workspace }
Etape "release" { cargo build --release -p pf-desktop }

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$nom = "PatternForge-$version-windows-x64"
$dossier = Join-Path $racine "dist\$nom"
Remove-Item (Join-Path $racine "dist") -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $dossier | Out-Null
Copy-Item target\release\patternforge.exe (Join-Path $dossier "PatternForge.exe")
Copy-Item README.md, README.fr.md, LICENSE, THIRD-PARTY.md $dossier
Copy-Item examples (Join-Path $dossier "examples") -Recurse
Copy-Item docs\GUIDE_UTILISATEUR.md $dossier
Compress-Archive -Path $dossier -DestinationPath (Join-Path $racine "dist\$nom.zip") -Force
$zip = Join-Path $racine "dist\$nom.zip"
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash
"$hash  $nom.zip" | Set-Content -Encoding ascii (Join-Path $racine "dist\$nom.zip.sha256")
Write-Host "SHA-256 : $hash"
Write-Host "Archive : dist\$nom.zip"
