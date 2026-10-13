//! Minimal dynamically loaded WinDivert 2.x ABI. No third-party example code.
use anyhow::{Context, Result, ensure};
use std::{ffi::{CString,c_void},path::PathBuf,sync::Arc};
use windows_sys::Win32::{Foundation::{HANDLE,HMODULE,INVALID_HANDLE_VALUE,FreeLibrary},System::LibraryLoader::*};

#[repr(C)]
#[derive(Clone,Copy)]
pub struct Address { pub timestamp:i64, pub flags:u32, reserved:u32, pub data:[u8;64] }
impl Default for Address { fn default()->Self{Self{timestamp:0,flags:0,reserved:0,data:[0;64]}} }
impl Address {
    pub fn inbound(&mut self){self.flags &= !(1<<17);}
}
type Open=unsafe extern "system" fn(*const u8,u32,i16,u64)->HANDLE;
type Recv=unsafe extern "system" fn(HANDLE,*mut c_void,u32,*mut u32,*mut Address)->i32;
type SendFn=unsafe extern "system" fn(HANDLE,*const c_void,u32,*mut u32,*const Address)->i32;
type Close=unsafe extern "system" fn(HANDLE)->i32;
type Shutdown=unsafe extern "system" fn(HANDLE,u32)->i32;
type Checksum=unsafe extern "system" fn(*mut c_void,u32,*mut Address,u64)->i32;
pub struct Api { module:HMODULE,open:Open,recv:Recv,send:SendFn,close:Close,shutdown:Shutdown,checksum:Checksum }
// WinDivert explicitly supports simultaneous calls from multiple threads.
unsafe impl Send for Api {} unsafe impl Sync for Api {}
impl Drop for Api{fn drop(&mut self){unsafe{FreeLibrary(self.module);}}}
pub fn directory()->Result<PathBuf>{Ok(std::env::current_exe()?.parent().context("Application directory missing")?.to_owned())}
impl Api {
    pub fn load()->Result<Arc<Self>>{unsafe{
        let path=directory()?.join("WinDivert.dll");
        let wide:Vec<u16>=path.as_os_str().to_string_lossy().encode_utf16().chain(Some(0)).collect();
        let module=LoadLibraryExW(wide.as_ptr(),std::ptr::null_mut(),LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR|LOAD_LIBRARY_SEARCH_SYSTEM32);
        ensure!(!module.is_null(),"Cannot load the bundled WinDivert.dll: {}",std::io::Error::last_os_error());
        let resolve=|name:&[u8]|GetProcAddress(module,name.as_ptr()).context("Incompatible WinDivert.dll");
        let result=(||Ok(Self{module,
            open:std::mem::transmute(resolve(b"WinDivertOpen\0")?),recv:std::mem::transmute(resolve(b"WinDivertRecv\0")?),
            send:std::mem::transmute(resolve(b"WinDivertSend\0")?),close:std::mem::transmute(resolve(b"WinDivertClose\0")?),
            shutdown:std::mem::transmute(resolve(b"WinDivertShutdown\0")?),checksum:std::mem::transmute(resolve(b"WinDivertHelperCalcChecksums\0")?)
        }))();
        match result {Ok(api)=>Ok(Arc::new(api)),Err(e)=>{FreeLibrary(module);Err(e)}}
    }}
    pub fn open(self:&Arc<Self>,filter:&str)->Result<Arc<Driver>>{
        let filter=CString::new(filter)?;
        let handle=unsafe{(self.open)(filter.as_ptr().cast(),0,17761,0)};
        ensure!(handle!=INVALID_HANDLE_VALUE,"Windows could not start the signed routing driver: {}. Install Out-Switch and use its background service; do not disable Windows security.",std::io::Error::last_os_error());
        Ok(Arc::new(Driver{api:self.clone(),handle}))
    }
}
pub struct Driver{api:Arc<Api>,handle:HANDLE}
unsafe impl Send for Driver {} unsafe impl Sync for Driver {}
impl Driver{
    pub fn recv(&self,packet:&mut [u8])->Result<(usize,Address)>{unsafe{
        let mut n=0;let mut address=Address::default();
        ensure!((self.api.recv)(self.handle,packet.as_mut_ptr().cast(),packet.len() as u32,&mut n,&mut address)!=0,"Packet receive failed: {}",std::io::Error::last_os_error());
        Ok((n as usize,address))
    }}
    pub fn send(&self,packet:&mut [u8],address:&mut Address)->Result<()>{unsafe{
        ensure!((self.api.checksum)(packet.as_mut_ptr().cast(),packet.len() as u32,address,0)!=0,"Invalid packet checksums");
        let mut sent=0;
        ensure!((self.api.send)(self.handle,packet.as_ptr().cast(),packet.len() as u32,&mut sent,address)!=0&&sent as usize==packet.len(),"Packet reinjection failed: {}",std::io::Error::last_os_error());Ok(())
    }}
    pub fn shutdown(&self){unsafe{(self.api.shutdown)(self.handle,3);}}
}
impl Drop for Driver{fn drop(&mut self){unsafe{(self.api.close)(self.handle);}}}
#[cfg(test)]mod tests{use super::*;#[test]fn address_matches_c_abi(){assert_eq!(std::mem::size_of::<Address>(),80);assert_eq!(std::mem::offset_of!(Address,data),16);}}
