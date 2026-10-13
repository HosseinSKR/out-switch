param([Parameter(Mandatory)][string]$MsiPath)
$ErrorActionPreference='Stop'
if (-not ('OutSwitchMsiPermissions' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class OutSwitchMsiPermissions {
    [DllImport("msi.dll", CharSet=CharSet.Unicode)] public static extern uint MsiOpenDatabaseW(string path, IntPtr mode, out uint database);
    [DllImport("msi.dll", CharSet=CharSet.Unicode)] public static extern uint MsiDatabaseOpenViewW(uint database, string query, out uint view);
    [DllImport("msi.dll")] public static extern uint MsiViewExecute(uint view, uint record);
    [DllImport("msi.dll")] public static extern uint MsiViewFetch(uint view, out uint record);
    [DllImport("msi.dll", CharSet=CharSet.Unicode)] public static extern uint MsiRecordGetStringW(uint record, uint field, StringBuilder value, ref uint length);
    [DllImport("msi.dll")] public static extern uint MsiCloseHandle(uint handle);
    [DllImport("advapi32.dll", CharSet=CharSet.Unicode, SetLastError=true)] public static extern bool ConvertStringSecurityDescriptorToSecurityDescriptorW(string text, uint revision, out IntPtr descriptor, out uint length);
    [DllImport("kernel32.dll")] public static extern IntPtr LocalFree(IntPtr memory);
}
'@
}
function Assert-MsiSuccess([uint32]$Code) { if ($Code -ne 0) { throw "MSI read failed: $Code" } }
$database=[uint32]0;$view=[uint32]0;$count=0
try {
    Assert-MsiSuccess ([OutSwitchMsiPermissions]::MsiOpenDatabaseW((Resolve-Path -LiteralPath $MsiPath).Path,[IntPtr]::Zero,[ref]$database))
    Assert-MsiSuccess ([OutSwitchMsiPermissions]::MsiDatabaseOpenViewW($database,'SELECT `SDDLText` FROM `MsiLockPermissionsEx`',[ref]$view))
    Assert-MsiSuccess ([OutSwitchMsiPermissions]::MsiViewExecute($view,0))
    while ($true) {
        $record=[uint32]0
        $code=[OutSwitchMsiPermissions]::MsiViewFetch($view,[ref]$record)
        if ($code -eq 259) { break }
        Assert-MsiSuccess $code
        try {
            $text=New-Object System.Text.StringBuilder 4096;$length=[uint32]4096
            Assert-MsiSuccess ([OutSwitchMsiPermissions]::MsiRecordGetStringW($record,1,$text,[ref]$length))
            $sddl=$text.ToString()
            if ($sddl.Contains('[') -or $sddl.Contains(']')) { throw 'MSI folder permissions contain an unsupported property reference; Windows Installer error 1943 would result.' }
            $descriptor=[IntPtr]::Zero;$bytes=[uint32]0
            if (-not [OutSwitchMsiPermissions]::ConvertStringSecurityDescriptorToSecurityDescriptorW($sddl,1,[ref]$descriptor,[ref]$bytes)) {
                throw "Packaged SDDL is invalid: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
            }
            [void][OutSwitchMsiPermissions]::LocalFree($descriptor)
            if ($sddl -ne 'D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)') { throw 'Unexpected service-data access policy. Review permissions before packaging.' }
            $count++
        } finally { if ($record) { [void][OutSwitchMsiPermissions]::MsiCloseHandle($record) } }
    }
    if ($count -ne 1) { throw "Expected one protected service-data folder; found $count." }
    Write-Host 'Packaged MSI permissions: valid Windows security descriptor; SYSTEM and administrators only.'
} finally {
    if ($view) { [void][OutSwitchMsiPermissions]::MsiCloseHandle($view) }
    if ($database) { [void][OutSwitchMsiPermissions]::MsiCloseHandle($database) }
}
