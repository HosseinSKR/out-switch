# Experimental compile check only; use Visual Studio/WDK for a supported release build.
param(
    [Parameter(Mandatory)][string]$WdkRoot,
    [Parameter(Mandatory)][string]$SdkRoot,
    [Parameter(Mandatory)][string]$LLVMDirectory,
    [string]$OutputDirectory=(Join-Path (Split-Path $PSScriptRoot) 'dist/unsigned-driver'),
    [string]$KitVersion='10.0.26100.0'
)
$ErrorActionPreference='Stop'
$driverRoot=Join-Path (Split-Path $PSScriptRoot) 'driver'
$kit=(Resolve-Path -LiteralPath $WdkRoot).Path
$sdk=(Resolve-Path -LiteralPath $SdkRoot).Path
$llvm=(Resolve-Path -LiteralPath $LLVMDirectory).Path
New-Item -ItemType Directory -Force $OutputDirectory | Out-Null
$output=(Resolve-Path -LiteralPath $OutputDirectory).Path
New-Item -ItemType Directory -Force "$output" | Out-Null
& "$llvm\clang.exe" --target=x86_64-pc-windows-msvc -c "$driverRoot\OutSwitch.c" -o "$output\OutSwitch.obj" -O2 -Wall -Wextra -Werror -fms-extensions -fms-compatibility -fno-builtin -fstack-protector-strong -mno-red-zone -DNDIS60 -D_AMD64_ -DAMD64 -D_WIN64 -D_KERNEL_MODE -D_WIN32_WINNT=0x0A00 -DNTDDI_VERSION=0x0A000000 -isystem "$kit\Include\$KitVersion\km" -isystem "$kit\Include\$KitVersion\km\crt" -isystem "$sdk\Include\$KitVersion\shared" -isystem "$sdk\Include\$KitVersion\ucrt"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
& "$llvm\ld.lld.exe" -flavor link /driver /release /subsystem:native /entry:GsDriverEntry /machine:x64 /nodefaultlib /dynamicbase /nxcompat /integritycheck /opt:ref /opt:icf /out:"$output\OutSwitchDriver.sys" "$output\OutSwitch.obj" "$kit\Lib\$KitVersion\km\x64\ntoskrnl.lib" "$kit\Lib\$KitVersion\km\x64\fwpkclnt.lib" "$kit\Lib\$KitVersion\km\x64\BufferOverflowK.lib"
exit $LASTEXITCODE
