use anyhow::{bail,Context,Result};
mod identity;
use outswitch_common::*;
use std::{ptr,time::{Duration,Instant}};
use windows_sys::Win32::{Foundation::*,Security::{*,Authorization::*},Storage::FileSystem::*,System::{Pipes::*,Threading::*}};

fn wide(s:&str)->Vec<u16>{s.encode_utf16().chain(Some(0)).collect()}
struct Handle(HANDLE);
impl Drop for Handle{fn drop(&mut self){unsafe{CloseHandle(self.0);}}}
fn error()->anyhow::Error{std::io::Error::last_os_error().into()}

pub fn current_sid()->Result<String>{unsafe{
    let mut token=ptr::null_mut();if OpenProcessToken(GetCurrentProcess(),TOKEN_QUERY,&mut token)==0{return Err(error());}let token=Handle(token);
    let mut len=0;GetTokenInformation(token.0,TokenUser,ptr::null_mut(),0,&mut len);
    let mut buf=vec![0u64;(len as usize+7)/8];
    if GetTokenInformation(token.0,TokenUser,buf.as_mut_ptr().cast(),len,&mut len)==0{return Err(error());}
    let user=&*(buf.as_ptr() as *const TOKEN_USER);let mut sid=ptr::null_mut();
    if ConvertSidToStringSidW(user.User.Sid,&mut sid)==0{return Err(error());}
    let mut n=0;while *sid.add(n)!=0{n+=1;}let result=String::from_utf16_lossy(std::slice::from_raw_parts(sid,n));LocalFree(sid.cast());Ok(result)
}}
fn valid_sid(s:&str)->bool{s.starts_with("S-1-")&&s.len()<192&&s.bytes().all(|b|b.is_ascii_digit()||b==b'-'||b==b'S')}

/// One local, owner-authorized request per connection. No impersonation or shell commands.
pub struct Server {handle:Handle}
impl Server {
    pub fn new(owner:&str)->Result<Self>{unsafe{
        anyhow::ensure!(valid_sid(owner),"Invalid authorized owner SID");
        let sddl=wide(&format!("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;0x12019b;;;{owner})"));
        let mut descriptor=ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.as_ptr(),1,&mut descriptor,ptr::null_mut())==0{return Err(error());}
        let sa=SECURITY_ATTRIBUTES{nLength:std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,lpSecurityDescriptor:descriptor,bInheritHandle:0};
        let handle=CreateNamedPipeW(wide(PIPE_NAME).as_ptr(),PIPE_ACCESS_DUPLEX|FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_MESSAGE|PIPE_READMODE_MESSAGE|PIPE_NOWAIT|PIPE_REJECT_REMOTE_CLIENTS,1,MAX_MESSAGE as u32,MAX_MESSAGE as u32,0,&sa);
        LocalFree(descriptor);
        if handle==INVALID_HANDLE_VALUE{return Err(error()).context("Cannot create local control pipe; another service may be running");}
        Ok(Self{handle:Handle(handle)})
    }}
    pub fn poll(&self,handler:impl FnOnce(Request)->Response)->Result<bool>{unsafe{
        if ConnectNamedPipe(self.handle.0,ptr::null_mut())==0 {
            let code=GetLastError();if code==ERROR_PIPE_LISTENING{return Ok(false);}if code!=ERROR_PIPE_CONNECTED&&code!=ERROR_NO_DATA{return Err(error());}
        }
        let result=(||{
            let data=receive(self.handle.0,Duration::from_millis(800))?;
            let request:Request=serde_json::from_slice(&data)?;let response=handler(request);let bytes=serde_json::to_vec(&response)?;
            anyhow::ensure!(bytes.len()<=MAX_MESSAGE,"Response exceeds IPC limit");
            let mut written=0;if WriteFile(self.handle.0,bytes.as_ptr(),bytes.len() as u32,&mut written,ptr::null_mut())==0{return Err(error());}
            anyhow::ensure!(written as usize==bytes.len(),"Incomplete response");
            // Wait a bounded interval for the peer to drain the response; never FlushFileBuffers (unbounded).
            let until=Instant::now()+Duration::from_millis(500);
            loop {let mut available=0;if PeekNamedPipe(self.handle.0,ptr::null_mut(),0,ptr::null_mut(),&mut available,ptr::null_mut())==0{break;}if Instant::now()>=until{break;}std::thread::sleep(Duration::from_millis(5));}
            Ok(true)
        })();
        DisconnectNamedPipe(self.handle.0);result
    }}
}
unsafe fn receive(handle:HANDLE,timeout:Duration)->Result<Vec<u8>>{
    let deadline=Instant::now()+timeout;let mut bytes=vec![0u8;MAX_MESSAGE];
    loop {
        let mut read=0;if ReadFile(handle,bytes.as_mut_ptr(),bytes.len() as u32,&mut read,ptr::null_mut())!=0{bytes.truncate(read as usize);return Ok(bytes);}
        let code=GetLastError();if code!=ERROR_NO_DATA&&code!=ERROR_PIPE_LISTENING{return Err(error());}
        if Instant::now()>=deadline{bail!("Local service request timed out");}std::thread::sleep(Duration::from_millis(10));
    }
}
pub fn request(request:&Request)->Result<Response>{unsafe{
    let name=wide(PIPE_NAME);let deadline=Instant::now()+Duration::from_secs(2);
    let handle=loop{
        // Anonymous SQOS prevents a pipe-squatting process from impersonating the user.
        let h=CreateFileW(name.as_ptr(),FILE_READ_DATA|FILE_WRITE_DATA|FILE_WRITE_ATTRIBUTES,0,ptr::null(),OPEN_EXISTING,SECURITY_SQOS_PRESENT|SECURITY_ANONYMOUS,ptr::null_mut());
        if h!=INVALID_HANDLE_VALUE{break Handle(h);}
        if GetLastError()!=ERROR_PIPE_BUSY||Instant::now()>deadline{return Err(error()).context("Out-Switch service is unavailable");}std::thread::sleep(Duration::from_millis(30));
    };
    let mut server_pid=0;
    if GetNamedPipeServerProcessId(handle.0,&mut server_pid)==0{return Err(error()).context("Cannot verify the local service");}
    let current=std::env::current_exe()?;
    let expected=if cfg!(test){current}else{current.with_file_name("OutSwitchService.exe")};
    let process=OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,0,server_pid);
    let actual=if process.is_null(){
        let code=GetLastError();
        if code!=ERROR_ACCESS_DENIED{return Err(error()).context("Cannot inspect the local service identity");}
        identity::verify_installed(server_pid,&expected.to_string_lossy()).map_err(|e|anyhow::anyhow!("Cannot verify the installed service: {e:#}"))?;
        None
    }else{
        let process=Handle(process);let mut path=vec![0u16;32768];let mut length=path.len() as u32;
        if QueryFullProcessImageNameW(process.0,0,path.as_mut_ptr(),&mut length)==0{
            let code=GetLastError();if code!=ERROR_ACCESS_DENIED{return Err(error()).context("Cannot read the local service executable");}
            identity::verify_installed(server_pid,&expected.to_string_lossy()).map_err(|e|anyhow::anyhow!("Cannot verify the installed service: {e:#}"))?;None
        }else{Some(String::from_utf16_lossy(&path[..length as usize]))}
    };
    if let Some(actual)=actual{anyhow::ensure!(outswitch_common::identity(&actual)==outswitch_common::identity(&expected.to_string_lossy()),"The pipe server is not the Out-Switch service beside this application");}
    let mode=PIPE_READMODE_MESSAGE|PIPE_NOWAIT;
    if SetNamedPipeHandleState(handle.0,&mode,ptr::null(),ptr::null())==0{return Err(error());}
    let data=serde_json::to_vec(request)?;anyhow::ensure!(data.len()<=MAX_MESSAGE,"Request too large");
    let mut written=0;if WriteFile(handle.0,data.as_ptr(),data.len() as u32,&mut written,ptr::null_mut())==0{return Err(error());}
    anyhow::ensure!(written as usize==data.len(),"Incomplete request");
    Ok(serde_json::from_slice(&receive(handle.0,Duration::from_secs(6))?)?)
}}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn rejects_sddl_injection(){assert!(!valid_sid("S-1-5-21)(A;;GA;;;WD)"));assert!(valid_sid("S-1-5-21-123-456-789-1001"));}
    #[test] fn owner_authorized_named_pipe_round_trip(){
        let (tx,rx)=std::sync::mpsc::channel();
        let server=std::thread::spawn(move||{
            let s=Server::new(&current_sid().unwrap()).unwrap();tx.send(()).unwrap();
            let until=Instant::now()+Duration::from_secs(5);
            loop{if s.poll(|r|{assert!(matches!(r,Request::Snapshot));Response{ok:true,message:"local-only".into(),snapshot:None}}).unwrap(){break;}assert!(Instant::now()<until);std::thread::sleep(Duration::from_millis(10));}
        });
        rx.recv().unwrap();let reply=request(&Request::Snapshot).unwrap();assert!(reply.ok);assert_eq!(reply.message,"local-only");server.join().unwrap();
    }
}
