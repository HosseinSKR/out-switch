$ErrorActionPreference='Stop'
$expected=Join-Path $PSScriptRoot 'OutSwitchService.exe'
if (Get-Service OutSwitch -ErrorAction SilentlyContinue | Where-Object Status -EQ Running) { throw 'An installed service is running. Use its administrator recovery script instead.' }
Get-Process OutSwitchService -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $expected } | Stop-Process
Write-Host 'Stopped the matching portable helper. Saved profiles are retained.'
