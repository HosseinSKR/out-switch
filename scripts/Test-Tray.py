import ctypes as c,subprocess,time,json,sys
from pathlib import Path
root=Path(__file__).resolve().parent.parent
u=c.WinDLL('user32',use_last_error=True)
u.GetWindowTextW.argtypes=[c.c_void_p,c.c_wchar_p,c.c_int]
u.GetWindowThreadProcessId.argtypes=[c.c_void_p,c.POINTER(c.c_ulong)]
u.PostMessageW.argtypes=[c.c_void_p,c.c_uint,c.c_size_t,c.c_ssize_t]
u.IsWindowVisible.argtypes=[c.c_void_p];u.IsIconic.argtypes=[c.c_void_p]
enum=c.WINFUNCTYPE(c.c_int,c.c_void_p,c.c_ssize_t)
u.EnumWindows.argtypes=[enum,c.c_ssize_t]
# Screenshot mode prevents changes to service and user settings; timeout leaves time for assertions.
p=subprocess.Popen([str(Path(sys.argv[1]).resolve()),'--screenshot',str(root/'tray-test.png')])
def windows():
    found={}
    @enum
    def callback(h,l):
        pid=c.c_ulong();u.GetWindowThreadProcessId(h,c.byref(pid))
        if pid.value==p.pid:
            name=c.create_unicode_buffer(128);u.GetWindowTextW(h,name,128);found[name.value]=h
        return 1
    u.EnumWindows(callback,0);return found
def wait(test,message):
    for _ in range(60):
        if test():return
        time.sleep(.05)
    raise AssertionError(message)
try:
    wait(lambda:all(n in windows() for n in ['OutSwitchTray','Out-Switch']),'windows not created')
    w=windows();main=w['Out-Switch'];tray=w['OutSwitchTray']
    # Real menu commands all pass through this same WM_COMMAND handler.
    u.PostMessageW(tray,0x111,4,0);wait(lambda:not u.IsWindowVisible(main),'Hide failed')
    u.PostMessageW(tray,0x111,1,0);wait(lambda:u.IsWindowVisible(main) and not u.IsIconic(main),'Open failed')
    time.sleep(.15);assert u.IsWindowVisible(main),'Open re-hid the window'
    u.PostMessageW(tray,0x111,4,0);wait(lambda:not u.IsWindowVisible(main),'second Hide failed')
    # VERSION_4 keyboard activation callback, notification ID in high word.
    u.PostMessageW(tray,0x8001,0,(1<<16)|1025);wait(lambda:u.IsWindowVisible(main),'keyboard open failed')
    u.PostMessageW(tray,0x111,4,0);wait(lambda:not u.IsWindowVisible(main),'third Hide failed')
    u.PostMessageW(tray,0x111,3,0);p.wait(timeout=5);assert p.returncode==0
    print('PASS: tray hide, open, stable restored visibility, keyboard open and exit while hidden.')
finally:
    if p.poll() is None:p.kill();p.wait()

