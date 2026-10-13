#Requires -RunAsAdministrator
param([Parameter(Mandatory)][string]$MsiPath)
$ErrorActionPreference='Stop'
if (Get-Service OutSwitch,OutSwitchDriver -ErrorAction SilentlyContinue) { throw 'Use a disposable clean Windows VM. Existing Out-Switch services were found.' }
if (Get-Process OutSwitch,OutSwitchService -ErrorAction SilentlyContinue) { throw 'An Out-Switch process is already running; use a clean VM.' }
$msi=(Resolve-Path -LiteralPath $MsiPath).Path
$logs=Split-Path $msi
$installed=$false
try {
    $install=Start-Process msiexec.exe -ArgumentList @('/i',('"'+$msi+'"'),'/qn','/norestart','/l*v',('"'+(Join-Path $logs 'install-test.log')+'"')) -WindowStyle Hidden -PassThru -Wait
    if ($install.ExitCode -notin @(0,3010)) { throw "MSI installation failed: $($install.ExitCode)" }
    $installed=$true
    $service=Get-Service OutSwitch
    if ($service.Status -ne 'Running') { throw 'Installed service is not running.' }
    $exe=Join-Path $env:ProgramFiles 'OutSwitch\OutSwitchService.exe'
    $response=& $exe --ipc-snapshot
    if ($LASTEXITCODE -ne 0) { throw 'Installed service IPC failed.' }
    $reply=$response | ConvertFrom-Json
    if (!$reply.ok -or !$reply.snapshot.service_connected) { throw 'Service did not return a connected snapshot.' }
    if (!$reply.snapshot.config.routing_enabled) { throw 'Fresh installation did not enable Network Control.' }
    if (@($reply.snapshot.config.profiles.Normal).Count -ne 0) { throw 'A fresh installation must have no custom routes.' }
    if (Get-Service OutSwitchDriver -ErrorAction SilentlyContinue) { throw 'The retired unsigned driver must not be installed.' }
    if(!(Test-Path -LiteralPath (Join-Path (Split-Path $exe) 'WinDivert64.sys'))){throw 'Signed driver payload is missing.'}
    & $exe --driver-check
    if($LASTEXITCODE -ne 0){throw 'Installed driver smoke check failed.'}
    Write-Host 'Installed service, authenticated IPC, routing-on default without custom routes and signed-driver loading verified.'
} finally {
    if ($installed) {
        $uninstall=Start-Process msiexec.exe -ArgumentList @('/x',('"'+$msi+'"'),'/qn','/norestart','/l*v',('"'+(Join-Path $logs 'uninstall-test.log')+'"')) -WindowStyle Hidden -PassThru -Wait
        if ($uninstall.ExitCode -notin @(0,3010)) { throw "MSI uninstall failed: $($uninstall.ExitCode)" }
        if (Get-Service OutSwitch -ErrorAction SilentlyContinue) { throw 'Service was not removed by uninstall.' }
        Write-Host 'Uninstall removed the installed service. Saved data is intentionally retained.'
    }
}
