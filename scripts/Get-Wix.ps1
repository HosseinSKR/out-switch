param([Parameter(Mandatory)][string]$Destination)
$ErrorActionPreference='Stop'
New-Item -ItemType Directory -Force $Destination | Out-Null
$destinationPath=(Resolve-Path -LiteralPath $Destination).Path
$archive=Join-Path $destinationPath 'wix314-binaries.zip'
Invoke-WebRequest 'https://github.com/wixtoolset/wix3/releases/download/wix3141rtm/wix314-binaries.zip' -OutFile $archive
$expected='6AC824E1642D6F7277D0ED7EA09411A508F6116BA6FAE0AA5F2C7DAA2FF43D31'
if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne $expected) { throw 'WiX archive checksum did not match the pinned release.' }
Expand-Archive -LiteralPath $archive -DestinationPath $destinationPath -Force
Write-Host 'WiX 3.14.1 extracted locally; no system installation performed.'
