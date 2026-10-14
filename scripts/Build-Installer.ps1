param(
    [Parameter(Mandatory)][string]$WixDirectory,
    [string]$PackageDirectory=(Split-Path $PSScriptRoot),
    [string]$OutputDirectory=(Join-Path (Split-Path $PSScriptRoot) 'dist'),
    [switch]$SkipMsiValidation
)
$ErrorActionPreference='Stop'
$source=(Resolve-Path -LiteralPath (Split-Path $PSScriptRoot)).Path
$wix=(Resolve-Path -LiteralPath $WixDirectory).Path
$package=(Resolve-Path -LiteralPath $PackageDirectory).Path
foreach ($file in @('OutSwitch.exe','OutSwitchService.exe','WinDivert.dll','WinDivert64.sys','README.md','LICENSE','THIRD_PARTY_NOTICES.md','licenses')) {
    if (!(Test-Path -LiteralPath (Join-Path $package $file))) { throw "Package is missing $file" }
}
New-Item -ItemType Directory -Force $OutputDirectory | Out-Null
$output=(Resolve-Path -LiteralPath $OutputDirectory).Path
$build=Join-Path $output 'installer-build'
New-Item -ItemType Directory -Force $build | Out-Null
& "$wix\heat.exe" dir "$package\licenses" -nologo -ag -srd -scom -sreg -cg DependencyLicenses -dr LICENSEDIR -var var.LicenseDir -out "$build\licenses.wxs"
if ($LASTEXITCODE -ne 0) { throw 'License harvesting failed' }
& "$wix\candle.exe" -ext "$wix\WixUtilExtension.dll" -nologo -arch x64 "-dPackageDir=$package" "-dSourceDir=$source" "-dLicenseDir=$package\licenses" -out "$build\" "$source\installer\OutSwitch.wxs" "$build\licenses.wxs"
if ($LASTEXITCODE -ne 0) { throw 'Installer compilation failed' }
$arguments=@('-ext',"$wix\WixUtilExtension.dll",'-nologo','-sacl','-ext',"$wix\WixUIExtension.dll",'-out',"$output\Out-Switch-0.6.0-x64.msi","$build\OutSwitch.wixobj","$build\licenses.wixobj")
if ($SkipMsiValidation) { $arguments += '-sval' }
$installerTemp=Join-Path $build 'temp'
New-Item -ItemType Directory -Force $installerTemp | Out-Null
$previousTemp=$env:TEMP;$previousTmp=$env:TMP;$previousWixTemp=$env:WIX_TEMP
try {
    $env:TEMP=$installerTemp;$env:TMP=$installerTemp;$env:WIX_TEMP=$installerTemp
    & "$wix\light.exe" @arguments
    if ($LASTEXITCODE -ne 0) { throw 'Installer linking or validation failed' }
} finally {
    $env:TEMP=$previousTemp;$env:TMP=$previousTmp;$env:WIX_TEMP=$previousWixTemp
}
& "$PSScriptRoot\Test-InstallerPermissions.ps1" -MsiPath "$output\Out-Switch-0.6.0-x64.msi"
Write-Host "Installer created: $output\Out-Switch-0.6.0-x64.msi"
Write-Host 'Includes signed WinDivert. A successful package build does not prove installation or live routing compatibility.'

