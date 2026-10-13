#Requires -RunAsAdministrator
param([string]$ApplicationDirectory=(Join-Path $env:ProgramFiles 'OutSwitch'),[string]$NetworkId)
$ErrorActionPreference='Stop'
$exe=Join-Path $ApplicationDirectory 'OutSwitchService.exe'
if(!(Test-Path -LiteralPath $exe)){throw 'Install Out-Switch first, or provide its extracted application directory.'}
if(Get-Service OutSwitch -ErrorAction SilentlyContinue){
    $snapshot=& $exe --ipc-snapshot | ConvertFrom-Json
    if($LASTEXITCODE -ne 0 -or !$snapshot.ok){throw 'Cannot check the installed service. Stop it before running an isolated test.'}
    if($snapshot.snapshot.config.routing_enabled){throw 'Turn network control OFF in Out-Switch before running this isolated test.'}
}
& $exe --driver-check
if($LASTEXITCODE -ne 0){throw 'Windows rejected the driver. Do not disable Windows security.'}
if(!$NetworkId){
    $networks=& $exe --ping-list | ConvertFrom-Json
    $connected=@($networks | Where-Object {$_.connected -and $_.gateways.Count -gt 0})
    if($connected.Count -eq 0){throw 'No connected internet adapter was found.'}
    foreach($network in $connected){
        & $exe --routing-test $network.id
        if($LASTEXITCODE -ne 0){throw "TCP/UDP relay test failed on $($network.name). Keep routing off until diagnosed."}
    }
}else{
    & $exe --routing-test $NetworkId
    if($LASTEXITCODE -ne 0){throw 'TCP/UDP relay test failed. Keep routing off until diagnosed.'}
}
Write-Host 'Basic driver and TCP/UDP relay tests passed. See docs/VALIDATION.md for the remaining release checks.'
