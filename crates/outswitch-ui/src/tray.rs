use std::{ptr,sync::{mpsc::Sender,OnceLock,atomic::{AtomicIsize,Ordering}}};
use windows_sys::Win32::{Foundation::*,System::LibraryLoader::GetModuleHandleW,UI::{Shell::*,WindowsAndMessaging::*}};
#[derive(Clone,Copy)]pub enum Action{Open,Pause,Exit,Ready}
static WINDOW:AtomicIsize=AtomicIsize::new(0);
static SENDER:OnceLock<(Sender<Action>,eframe::egui::Context)>=OnceLock::new();
fn wide(s:&str)->Vec<u16>{s.encode_utf16().chain(Some(0)).collect()}
fn send(a:Action){if let Some((tx,ctx))=SENDER.get(){let _=tx.send(a);ctx.request_repaint();}}
unsafe extern "system" fn window(hwnd:HWND,msg:u32,wp:WPARAM,lp:LPARAM)->LRESULT{
    if msg==WM_DESTROY{let mut icon:NOTIFYICONDATAW=std::mem::zeroed();icon.cbSize=std::mem::size_of::<NOTIFYICONDATAW>() as u32;icon.hWnd=hwnd;icon.uID=1;Shell_NotifyIconW(NIM_DELETE,&icon);PostQuitMessage(0);return 0;}
    if msg==WM_APP+1 {
        match lp as u32 {
            WM_LBUTTONDBLCLK=>send(Action::Open),
            WM_RBUTTONUP=>{
                let menu=CreatePopupMenu();for(id,text)in [(1,"Open Out-Switch"),(2,"Stop all routing"),(3,"Exit desktop app")]{AppendMenuW(menu,MF_STRING,id,wide(text).as_ptr());}
                let mut point=POINT{x:0,y:0};GetCursorPos(&mut point);SetForegroundWindow(hwnd);
                let chosen=TrackPopupMenu(menu,TPM_RETURNCMD|TPM_NONOTIFY,point.x,point.y,0,hwnd,ptr::null());DestroyMenu(menu);
                match chosen{1=>send(Action::Open),2=>send(Action::Pause),3=>send(Action::Exit),_=>{}}PostMessageW(hwnd,WM_NULL,0,0);
            },_=>{}
        }return 0;
    }DefWindowProcW(hwnd,msg,wp,lp)
}
pub fn start(tx:Sender<Action>,ctx:eframe::egui::Context){
    let _=SENDER.set((tx,ctx));std::thread::spawn(||unsafe{
        let instance=GetModuleHandleW(ptr::null());let name=wide("OutSwitchTray");let mut class:WNDCLASSW=std::mem::zeroed();class.hInstance=instance;class.lpszClassName=name.as_ptr();class.lpfnWndProc=Some(window);RegisterClassW(&class);
        let hwnd=CreateWindowExW(0,name.as_ptr(),name.as_ptr(),0,0,0,0,0,ptr::null_mut(),ptr::null_mut(),instance,ptr::null());if hwnd.is_null(){return;}
        let mut icon:NOTIFYICONDATAW=std::mem::zeroed();icon.cbSize=std::mem::size_of::<NOTIFYICONDATAW>() as u32;icon.hWnd=hwnd;icon.uID=1;icon.uFlags=NIF_MESSAGE|NIF_ICON|NIF_TIP;icon.uCallbackMessage=WM_APP+1;icon.hIcon=LoadIconW(instance,1usize as *const u16);
        let tip=wide("Out-Switch — double-click to open");icon.szTip[..tip.len()].copy_from_slice(&tip);if Shell_NotifyIconW(NIM_ADD,&icon)!=0{WINDOW.store(hwnd as isize,Ordering::Relaxed);send(Action::Ready);}
        let mut message:MSG=std::mem::zeroed();while GetMessageW(&mut message,ptr::null_mut(),0,0)>0{TranslateMessage(&message);DispatchMessageW(&message);}Shell_NotifyIconW(NIM_DELETE,&icon);
    });
}
pub fn shutdown(){let hwnd=WINDOW.swap(0,Ordering::Relaxed) as HWND;if !hwnd.is_null(){unsafe{SendMessageTimeoutW(hwnd,WM_CLOSE,0,0,SMTO_ABORTIFHUNG,200,ptr::null_mut());}}}
