use std::{ptr,sync::{mpsc::Sender,OnceLock,atomic::{AtomicIsize,AtomicU32,Ordering}}};
use windows_sys::Win32::{Foundation::*,System::{LibraryLoader::GetModuleHandleW,Threading::GetCurrentProcessId},UI::{Shell::*,WindowsAndMessaging::*}};
#[derive(Clone,Copy)]pub enum Action{Open,Hide,Pause,Updates,Exit,Ready}
static WINDOW:AtomicIsize=AtomicIsize::new(0);
static TASKBAR:AtomicU32=AtomicU32::new(0);
static STATUS:AtomicU32=AtomicU32::new(0);
const NIN_KEYSELECT:u32=NIN_SELECT|1;
static SENDER:OnceLock<(Sender<Action>,eframe::egui::Context)>=OnceLock::new();
fn wide(s:&str)->Vec<u16>{s.encode_utf16().chain(Some(0)).collect()}
pub fn status(connected:bool,enabled:bool){STATUS.store(if !connected{0}else if enabled{2}else{1},Ordering::Relaxed);}
unsafe extern "system" fn find_main(hwnd:HWND,data:LPARAM)->BOOL{
    let mut pid=0;GetWindowThreadProcessId(hwnd,&mut pid);
    if pid==GetCurrentProcessId(){let mut name=[0u16;128];let len=GetWindowTextW(hwnd,name.as_mut_ptr(),128);if String::from_utf16_lossy(&name[..len as usize])=="Out-Switch"{*(data as *mut HWND)=hwnd;return 0;}}1
}
fn send(action:Action){
    // A hidden winit window does not necessarily redraw on request_repaint.
    // Restore it through Win32 first so queued actions are always consumed.
    if !matches!(action,Action::Ready){unsafe{let mut main:HWND=ptr::null_mut();EnumWindows(Some(find_main),&mut main as *mut HWND as LPARAM);if !main.is_null(){ShowWindowAsync(main,SW_RESTORE);if matches!(action,Action::Open|Action::Updates){SetForegroundWindow(main);}}}}
    if let Some((tx,ctx))=SENDER.get(){let _=tx.send(action);ctx.request_repaint();}
}
unsafe fn icon(hwnd:HWND,operation:u32)->bool{
    let mut data:NOTIFYICONDATAW=std::mem::zeroed();data.cbSize=std::mem::size_of::<NOTIFYICONDATAW>() as u32;data.hWnd=hwnd;data.uID=1;
    data.uFlags=NIF_MESSAGE|NIF_ICON|NIF_TIP|NIF_SHOWTIP;data.uCallbackMessage=WM_APP+1;data.hIcon=LoadIconW(GetModuleHandleW(ptr::null()),1usize as *const u16);
    let tip=wide("Out-Switch · Click to open");data.szTip[..tip.len()].copy_from_slice(&tip);
    let ok=Shell_NotifyIconW(operation,&data)!=0;
    if operation==NIM_ADD&&ok{data.Anonymous.uVersion=NOTIFYICON_VERSION_4;Shell_NotifyIconW(NIM_SETVERSION,&data);}ok
}
unsafe extern "system" fn window(hwnd:HWND,msg:u32,wp:WPARAM,lp:LPARAM)->LRESULT{
    if msg==WM_COMMAND{match wp&0xffff{1=>send(Action::Open),2=>send(Action::Pause),3=>send(Action::Exit),4=>send(Action::Hide),5=>send(Action::Updates),_=>{}}return 0;}
    if msg==TASKBAR.load(Ordering::Relaxed)&&msg!=0{if icon(hwnd,NIM_ADD){send(Action::Ready);}return 0;}
    if msg==WM_DESTROY{icon(hwnd,NIM_DELETE);PostQuitMessage(0);return 0;}
    if msg==WM_APP+1{
        match (lp as u32)&0xffff{
            NIN_SELECT|NIN_KEYSELECT|WM_LBUTTONDBLCLK=>send(Action::Open),
            WM_CONTEXTMENU|WM_RBUTTONUP=>{
                let menu=CreatePopupMenu();
                let header=match STATUS.load(Ordering::Relaxed){2=>"Out-Switch · Network Control ON",1=>"Out-Switch · Network Control OFF",_=>"Out-Switch · Reconnecting…"};
                AppendMenuW(menu,MF_STRING|MF_DISABLED,0,wide(header).as_ptr());AppendMenuW(menu,MF_SEPARATOR,0,ptr::null());
                for(id,text)in [(1,"Open Out-Switch"),(4,"Hide window"),(5,"Check for updates")]{AppendMenuW(menu,MF_STRING,id,wide(text).as_ptr());}
                AppendMenuW(menu,MF_SEPARATOR,0,ptr::null());
                AppendMenuW(menu,MF_STRING,2,wide("Stop all routing").as_ptr());AppendMenuW(menu,MF_STRING,3,wide("Exit desktop app").as_ptr());
                let mut point=POINT{x:0,y:0};GetCursorPos(&mut point);SetForegroundWindow(hwnd);
                let chosen=TrackPopupMenu(menu,TPM_RETURNCMD|TPM_NONOTIFY,point.x,point.y,0,hwnd,ptr::null());DestroyMenu(menu);
                if chosen!=0{PostMessageW(hwnd,WM_COMMAND,chosen as usize,0);}
                PostMessageW(hwnd,WM_NULL,0,0);
            },_=>{}
        }return 0;
    }DefWindowProcW(hwnd,msg,wp,lp)
}
pub fn start(tx:Sender<Action>,ctx:eframe::egui::Context){
    let _=SENDER.set((tx,ctx));std::thread::spawn(||unsafe{
        let instance=GetModuleHandleW(ptr::null());let name=wide("OutSwitchTray");let mut class:WNDCLASSW=std::mem::zeroed();class.hInstance=instance;class.lpszClassName=name.as_ptr();class.lpfnWndProc=Some(window);RegisterClassW(&class);
        TASKBAR.store(RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()),Ordering::Relaxed);
        let hwnd=CreateWindowExW(0,name.as_ptr(),name.as_ptr(),0,0,0,0,0,ptr::null_mut(),ptr::null_mut(),instance,ptr::null());if hwnd.is_null(){return;}
        WINDOW.store(hwnd as isize,Ordering::Relaxed);if icon(hwnd,NIM_ADD){send(Action::Ready);}
        let mut message:MSG=std::mem::zeroed();while GetMessageW(&mut message,ptr::null_mut(),0,0)>0{TranslateMessage(&message);DispatchMessageW(&message);}
    });
}
pub fn shutdown(){let hwnd=WINDOW.swap(0,Ordering::Relaxed) as HWND;if !hwnd.is_null(){unsafe{SendMessageTimeoutW(hwnd,WM_CLOSE,0,0,SMTO_ABORTIFHUNG,200,ptr::null_mut());}}}
