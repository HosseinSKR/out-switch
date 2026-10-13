//! Verify a LocalSystem pipe server without requiring access to its process object.
use anyhow::{ensure, Context, Result};
use outswitch_common::identity;
use std::ptr;
use windows_sys::Win32::System::Services::*;

struct ServiceHandle(SC_HANDLE);
impl Drop for ServiceHandle {
    fn drop(&mut self) { unsafe { CloseServiceHandle(self.0); } }
}
fn os_error() -> anyhow::Error { std::io::Error::last_os_error().into() }
fn wide(s: &str) -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() }

fn valid_registration(command: &str, account: &str, expected: &str) -> bool {
    let command = command.trim();
    let executable = if command.len()>=2 && command.starts_with('"') && command.ends_with('"') {
        &command[1..command.len()-1]
    } else { command };
    !executable.contains('"') && identity(executable) == identity(expected)
        && (account.eq_ignore_ascii_case("LocalSystem") || account.eq_ignore_ascii_case(r"NT AUTHORITY\SYSTEM"))
}
fn valid_status(state: u32, kind: u32, pid: u32, server_pid: u32) -> bool {
    state == SERVICE_RUNNING && kind == SERVICE_WIN32_OWN_PROCESS && pid != 0 && pid == server_pid
}
unsafe fn status(service: SC_HANDLE) -> Result<SERVICE_STATUS_PROCESS> {
    let mut result: SERVICE_STATUS_PROCESS = std::mem::zeroed();
    let mut size = 0;
    if QueryServiceStatusEx(service, SC_STATUS_PROCESS_INFO, (&mut result as *mut SERVICE_STATUS_PROCESS).cast(),
        std::mem::size_of::<SERVICE_STATUS_PROCESS>() as u32, &mut size) == 0 {
        return Err(os_error()).context("Cannot read the installed service status");
    }
    Ok(result)
}
unsafe fn config_string(buffer: &[u64], value: *const u16) -> Result<String> {
    let start = buffer.as_ptr() as usize;
    let end = start + buffer.len()*8;
    let address = value as usize;
    ensure!(address >= start && address < end && (address-start)%2 == 0, "Invalid service configuration pointer");
    let text = std::slice::from_raw_parts(value, (end-address)/2);
    let length = text.iter().position(|v|*v == 0).context("Unterminated service configuration")?;
    Ok(String::from_utf16_lossy(&text[..length]))
}

/// The SCM is authoritative for the installed service's PID and binary path.
/// Only read access is requested; no process/pipe/service ACL is broadened.
pub(super) fn verify_installed(server_pid: u32, expected: &str) -> Result<()> { unsafe {
    let manager = OpenSCManagerW(ptr::null(), ptr::null(), SC_MANAGER_CONNECT);
    if manager.is_null() { return Err(os_error()).context("Cannot read Windows Service Manager"); }
    let manager = ServiceHandle(manager);
    let service = OpenServiceW(manager.0, wide("OutSwitch").as_ptr(), SERVICE_QUERY_CONFIG | SERVICE_QUERY_STATUS);
    if service.is_null() { return Err(os_error()).context("Cannot read the installed Out-Switch service"); }
    let service = ServiceHandle(service);
    let before = status(service.0)?;
    ensure!(valid_status(before.dwCurrentState, before.dwServiceType, before.dwProcessId, server_pid),
        "The pipe server is not the running installed Out-Switch service");

    let mut needed = 0;
    QueryServiceConfigW(service.0, ptr::null_mut(), 0, &mut needed);
    ensure!(needed as usize >= std::mem::size_of::<QUERY_SERVICE_CONFIGW>() && needed <= 8192,
        "Cannot read the installed service configuration");
    let mut buffer = vec![0u64;(needed as usize+7)/8];
    if QueryServiceConfigW(service.0, buffer.as_mut_ptr().cast(), (buffer.len()*8) as u32, &mut needed) == 0 {
        return Err(os_error()).context("Cannot read the installed service executable");
    }
    let config = &*buffer.as_ptr().cast::<QUERY_SERVICE_CONFIGW>();
    let executable = config_string(&buffer,config.lpBinaryPathName)?;
    let account = config_string(&buffer,config.lpServiceStartName)?;
    ensure!(valid_registration(&executable,&account,expected) && config.dwServiceType == SERVICE_WIN32_OWN_PROCESS,
        "The registered service executable or account does not match this installation");
    let after = status(service.0)?;
    ensure!(valid_status(after.dwCurrentState,after.dwServiceType,after.dwProcessId,server_pid),
        "The installed service restarted during identity verification; retry the request");
    Ok(())
}}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn accepts_exact_registered_service_with_quotes_and_case() {
        assert!(valid_registration(r#""C:\Program Files\OutSwitch\OutSwitchService.exe""#,"LocalSystem",r"c:\program files\outswitch\outswitchservice.exe"));
        assert!(valid_registration(r"C:\App\OutSwitchService.exe",r"NT AUTHORITY\SYSTEM",r"C:\App\OutSwitchService.exe"));
    }
    #[test] fn rejects_other_binary_arguments_and_account() {
        let expected=r"C:\App\OutSwitchService.exe";
        for command in ["", "\"", r"C:\Other\OutSwitchService.exe",r#""C:\App\OutSwitchService.exe" --spoof"#,r#""C:\App\OutSwitchService.exe" "other""#] {
            assert!(!valid_registration(command,"LocalSystem",expected));
        }
        assert!(!valid_registration(expected,"LocalService",expected));
    }
    #[test] fn rejects_wrong_pid_stopped_and_shared_service() {
        assert!(valid_status(SERVICE_RUNNING,SERVICE_WIN32_OWN_PROCESS,42,42));
        assert!(!valid_status(SERVICE_RUNNING,SERVICE_WIN32_OWN_PROCESS,43,42));
        assert!(!valid_status(SERVICE_STOPPED,SERVICE_WIN32_OWN_PROCESS,42,42));
        assert!(!valid_status(SERVICE_RUNNING,SERVICE_WIN32_SHARE_PROCESS,42,42));
        assert!(!valid_status(SERVICE_RUNNING,SERVICE_WIN32_OWN_PROCESS,0,0));
    }
}
