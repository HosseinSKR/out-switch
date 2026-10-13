$ErrorActionPreference='SilentlyContinue'
[Console]::OutputEncoding=New-Object System.Text.UTF8Encoding($false)
$found=@{}
function Add-Executable([string]$target,[string]$title) {
    $target=[Environment]::ExpandEnvironmentVariables($target).Trim('"')
    if ($target -notmatch '^[A-Za-z]:\\' -or [IO.Path]::GetExtension($target) -ine '.exe') { return }
    if (-not [IO.File]::Exists($target) -or $found.Count -ge 1000) { return }
    if ([string]::IsNullOrWhiteSpace($title)) { $title=[IO.Path]::GetFileNameWithoutExtension($target) }
    if ($title.Length -gt 256) { $title=$title.Substring(0,256) }
    $found[$target.ToLowerInvariant()]=[pscustomobject]@{path=$target;name=$title}
}
# App Paths registrations, including per-user and 32-bit applications.
foreach ($registryPath in @('HKCU:\Software\Microsoft\Windows\CurrentVersion\App Paths','HKLM:\Software\Microsoft\Windows\CurrentVersion\App Paths','HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\App Paths')) {
    Get-ChildItem -LiteralPath $registryPath | ForEach-Object { Add-Executable ($_.GetValue('')) '' }
}
# Read existing shortcuts only; never launch or save them.
$shell=New-Object -ComObject WScript.Shell
foreach ($folder in @([Environment]::GetFolderPath('Programs'),[Environment]::GetFolderPath('CommonPrograms'),[Environment]::GetFolderPath('DesktopDirectory'),[Environment]::GetFolderPath('CommonDesktopDirectory'))) {
    if ($folder) {
        Get-ChildItem -LiteralPath $folder -Filter *.lnk -File -Recurse | Select-Object -First 1000 | ForEach-Object {
            $link=$shell.CreateShortcut($_.FullName)
            Add-Executable $link.TargetPath $_.BaseName
            if ($link) { [void][Runtime.InteropServices.Marshal]::ReleaseComObject($link) }
        }
    }
}
if ($shell) { [void][Runtime.InteropServices.Marshal]::ReleaseComObject($shell) }
ConvertTo-Json -InputObject @($found.Values | Sort-Object name) -Compress
