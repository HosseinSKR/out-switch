#Requires -RunAsAdministrator
param([Parameter(Mandatory)][string]$PackageDirectory,[Parameter(Mandatory)][string]$OwnerSid)
$ErrorActionPreference='Stop'
throw 'Retired custom-driver installer. Use the Out-Switch 0.4 MSI, which includes the signed WinDivert component.'
if ($OwnerSid -notmatch '^S-1-5-21-(\d+-){3}\d+$') { throw 'Supply the authorized desktop user SID from whoami /user.' }
$package=(Resolve-Path -LiteralPath $PackageDirectory).Path
$driver=Join-Path $package 'OutSwitchDriver.sys'
foreach ($name in @('OutSwitch.exe','OutSwitchService.exe','OutSwitchDriver.sys')) { if (!(Test-Path -LiteralPath (Join-Path $package $name))) { throw "Missing $name. No installation performed." } }
$signature=Get-AuthenticodeSignature -LiteralPath $driver
if ($signature.Status -ne 'Valid') { throw 'A signed driver is required. This script does not disable Windows driver-signature enforcement.' }
if (Get-Service OutSwitch,OutSwitchDriver -ErrorAction SilentlyContinue) { throw 'An Out-Switch service already exists. Stop and review the existing installation before replacing it.' }
$destination=Join-Path $env:ProgramFiles 'OutSwitch'
$data=Join-Path $env:ProgramData 'OutSwitch'
New-Item -ItemType Directory -Force $destination,$data | Out-Null
& icacls.exe $destination /inheritance:r /grant:r '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-544:(OI)(CI)F' '*S-1-5-32-545:(OI)(CI)RX' | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Could not protect application directory.' }
& icacls.exe $data /inheritance:r /grant:r '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-544:(OI)(CI)F' "*${OwnerSid}:(OI)(CI)R" | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Could not protect data directory.' }
foreach ($name in @('OutSwitch.exe','OutSwitchService.exe','OutSwitchDriver.sys')) { Copy-Item -LiteralPath (Join-Path $package $name) -Destination $destination }
Set-Content -LiteralPath (Join-Path $data 'owner.sid') -Value $OwnerSid -Encoding ascii
$driverPath=Join-Path $destination 'OutSwitchDriver.sys'
$servicePath=Join-Path $destination 'OutSwitchService.exe'
& sc.exe create OutSwitchDriver type= kernel start= demand binPath= "`"$driverPath`""
if ($LASTEXITCODE -ne 0) { throw 'Driver service registration failed.' }
& sc.exe create OutSwitch type= own start= auto binPath= "`"$servicePath`"" depend= BFE/OutSwitchDriver obj= LocalSystem
if ($LASTEXITCODE -ne 0) { & sc.exe delete OutSwitchDriver; throw 'Application service registration failed; driver registration rolled back.' }
try { Start-Service OutSwitch } catch { throw 'Windows could not start the driver/service. Check signing and Event Viewer; no security settings were changed.' }
Write-Host 'Installed. Launch OutSwitch.exe as the authorized standard user. Routing remains disabled until explicitly enabled.'
