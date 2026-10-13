#Requires -RunAsAdministrator
$ErrorActionPreference='Stop'
Stop-Service OutSwitch -ErrorAction SilentlyContinue
Stop-Service OutSwitchDriver -ErrorAction SilentlyContinue
foreach ($name in @('OutSwitch','OutSwitchDriver')) {
    $service=Get-Service $name -ErrorAction SilentlyContinue
    if ($service -and $service.Status -ne 'Stopped') { throw "$name is still running; aborting removal." }
    if ($service) { & sc.exe delete $name; if ($LASTEXITCODE -ne 0) { throw "Could not unregister $name" } }
}
Write-Host 'Out-Switch services unregistered. Program files and local data are retained for manual review. No unrelated network settings were removed.'
