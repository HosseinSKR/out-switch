$ErrorActionPreference='Stop'
$serviceExe=Join-Path $PSScriptRoot 'OutSwitchService.exe'
$uiExe=Join-Path $PSScriptRoot 'OutSwitch.exe'
if (!(Test-Path -LiteralPath $serviceExe) -or !(Test-Path -LiteralPath $uiExe)) { throw 'Keep this launcher alongside both Out-Switch executables.' }
# The desktop app starts and manages its own helper when no service is installed.
Start-Process -FilePath $uiExe
