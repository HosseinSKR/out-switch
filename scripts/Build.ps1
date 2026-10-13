param([switch]$Test)
$ErrorActionPreference='Stop'
Push-Location (Split-Path $PSScriptRoot)
try {
    cargo build --release --locked
    if ($LASTEXITCODE -ne 0) { throw 'Rust build failed.' }
    if ($Test) { cargo test --workspace --locked; if ($LASTEXITCODE -ne 0) { throw 'Tests failed.' } }
    New-Item -ItemType Directory -Force dist | Out-Null
    Copy-Item target\release\OutSwitch.exe,target\release\OutSwitchService.exe dist
    Copy-Item scripts\Start-OutSwitch.cmd,scripts\Start-Portable.ps1,scripts\Stop-Portable-Service.ps1 dist
    Write-Host 'Built desktop application and service. Driver must be built and signed separately.'
} finally { Pop-Location }
