param([string]$Destination=(Split-Path $PSScriptRoot),[string]$ArchiveDirectory)
$ErrorActionPreference='Stop'
New-Item -ItemType Directory -Force $Destination | Out-Null
$destinationPath=(Resolve-Path -LiteralPath $Destination).Path
$stage=Join-Path $destinationPath 'packages\windivert'
New-Item -ItemType Directory -Force $stage | Out-Null
$packages=@(
    @{Name='WinDivert-2.2.2-A.zip';Hash='63CB41763BB4B20F600B6DE04E991A9C2BE73279E317D4D82F237B150C5F3F15'},
    @{Name='WinDivert-2.2.2-Source.zip';Hash='8E904FBEEF2A6180FFC533E9B636AFD14282AF7510E421A9CB381D978302ACD9'}
)
foreach($package in $packages){
    $archive=Join-Path $stage $package.Name
    if($ArchiveDirectory){Copy-Item -LiteralPath (Join-Path $ArchiveDirectory $package.Name) -Destination $archive -Force}
    elseif(!(Test-Path -LiteralPath $archive)){
        $bundled=Join-Path $destinationPath ('licenses\WinDivert-2.2.2\'+$package.Name)
        if(Test-Path -LiteralPath $bundled){Copy-Item -LiteralPath $bundled -Destination $archive}
        else{
            $urls=@('https://reqrypt.org/download/'+$package.Name)
            if($package.Name -eq 'WinDivert-2.2.2-A.zip'){$urls=@('https://github.com/basil00/WinDivert/releases/download/v2.2.2/WinDivert-2.2.2-A.zip')+$urls}
            $downloaded=$false;$partial=$archive+'.partial'
            foreach($url in $urls){
                try{
                    Invoke-WebRequest -Uri $url -OutFile $partial -TimeoutSec 60
                    if((Get-FileHash -LiteralPath $partial -Algorithm SHA256).Hash -ne $package.Hash){throw 'Downloaded archive checksum mismatch'}
                    Move-Item -LiteralPath $partial -Destination $archive -Force
                    $downloaded=$true;break
                }catch{Write-Warning "Could not retrieve pinned $($package.Name) from ${url}: $_"}
                finally{if(Test-Path -LiteralPath $partial){Remove-Item -LiteralPath $partial -Force}}
            }
            if(!$downloaded){throw "Could not download verified archive: $($package.Name)"}
        }
    }
    if((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne $package.Hash){throw "Official archive hash mismatch: $($package.Name)"}
}
Expand-Archive -LiteralPath (Join-Path $stage 'WinDivert-2.2.2-A.zip') -DestinationPath $stage -Force
$binary=Join-Path $stage 'WinDivert-2.2.2-A'
$driver=Join-Path $binary 'x64\WinDivert64.sys'
if((Get-AuthenticodeSignature -LiteralPath $driver).Status -ne 'Valid'){throw 'WinDivert driver signature verification failed'}
Copy-Item -LiteralPath $driver -Destination (Join-Path $destinationPath 'WinDivert64.sys') -Force
Copy-Item -LiteralPath (Join-Path $binary 'x64\WinDivert.dll') -Destination (Join-Path $destinationPath 'WinDivert.dll') -Force
$licenseDir=Join-Path $destinationPath 'licenses\WinDivert-2.2.2'
New-Item -ItemType Directory -Force $licenseDir | Out-Null
Copy-Item -LiteralPath (Join-Path $binary 'LICENSE') -Destination $licenseDir -Force
Copy-Item -LiteralPath (Join-Path $stage 'WinDivert-2.2.2-Source.zip') -Destination $licenseDir -Force
@'
WinDivert 2.2.2-A, Copyright (c) Basil and contributors.
Distributed under the LGPL version 3 or later option, dynamically loaded.
Official distribution: https://reqrypt.org/windivert.html
The unmodified matching source and build scripts are in WinDivert-2.2.2-Source.zip.
The DLL is separately replaceable. Out-Switch does not prohibit reverse engineering
for debugging modifications to this library. Windows driver signing policy still applies.
No GPL example program is linked into Out-Switch.
'@ | Set-Content -LiteralPath (Join-Path $licenseDir 'NOTICE.txt') -Encoding utf8
Write-Host 'Verified official archive hashes and Authenticode; copied x64 driver, DLL, license and matching source.'
