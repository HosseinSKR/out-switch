#Requires -RunAsAdministrator
$ErrorActionPreference='Stop'
Stop-Service -Name OutSwitch -ErrorAction SilentlyContinue
$service=Get-Service OutSwitch -ErrorAction SilentlyContinue
if ($service -and $service.Status -ne 'Stopped') { throw 'Service did not stop. Inspect its state in Services.' }
Write-Host 'Service stopped. Its dynamic WFP filters are released by Windows. Existing sockets may need to reconnect.'
