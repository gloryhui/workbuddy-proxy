#define UNICODE
#define _UNICODE
#include <windows.h>
#include <shellapi.h>
#include <string.h>
extern void wb_gpui_open(void);
extern void wb_gpui_stop(void);
extern int wb_gpui_exit_requested(void);
extern void wb_frontend_ready(void);
extern void wb_frontend_closing(void);
extern char *wb_command(const char *);
extern char *wb_value(const char *);
extern void wb_free(char *);
static HWND hostWindow;
static NOTIFYICONDATAW hostIcon;
static BOOL hostChinese;
static void hostQuit(void){wb_gpui_stop();wb_free(wb_command("quit"));DestroyWindow(hostWindow);}
static LRESULT CALLBACK hostProcedure(HWND h,UINT msg,WPARAM w,LPARAM l){
    if(msg==WM_APP+3){wb_gpui_open();return 0;}
    if(msg==WM_APP+1){if(l==WM_LBUTTONUP||l==WM_LBUTTONDBLCLK){wb_gpui_open();return 0;}if(l==WM_RBUTTONUP||l==WM_CONTEXTMENU){POINT p;GetCursorPos(&p);SetForegroundWindow(h);HMENU menu=CreatePopupMenu();AppendMenuW(menu,MF_STRING,1,hostChinese?L"设置":L"Settings");AppendMenuW(menu,MF_SEPARATOR,0,NULL);AppendMenuW(menu,MF_STRING,2,hostChinese?L"退出":L"Quit");TrackPopupMenu(menu,TPM_RIGHTBUTTON,p.x,p.y,0,h,NULL);DestroyMenu(menu);return 0;}}
    if(msg==WM_COMMAND){if(LOWORD(w)==1)wb_gpui_open();if(LOWORD(w)==2)hostQuit();return 0;}
    if(msg==WM_TIMER){if(wb_gpui_exit_requested())hostQuit();return 0;}
    if(msg==WM_APP+4||msg==WM_CLOSE){hostQuit();return 0;}
    if(msg==WM_DESTROY){wb_frontend_closing();KillTimer(h,1);Shell_NotifyIconW(NIM_DELETE,&hostIcon);hostWindow=NULL;PostQuitMessage(0);return 0;}
    if(msg==RegisterWindowMessageW(L"TaskbarCreated")){Shell_NotifyIconW(NIM_ADD,&hostIcon);return 0;}
    return DefWindowProcW(h,msg,w,l);
}
void wb_gpui_host_run(void){
    HINSTANCE instance=GetModuleHandleW(NULL);char *language=wb_value("language");hostChinese=strcmp(language,"zh")==0||(strcmp(language,"auto")==0&&PRIMARYLANGID(GetUserDefaultUILanguage())==LANG_CHINESE);wb_free(language);
    WNDCLASSW cls={0};cls.lpfnWndProc=hostProcedure;cls.hInstance=instance;cls.lpszClassName=L"WorkBuddyProxyWindow";cls.hIcon=LoadIconW(instance,MAKEINTRESOURCEW(1));RegisterClassW(&cls);
    hostWindow=CreateWindowExW(0,cls.lpszClassName,L"WorkBuddy Proxy",WS_OVERLAPPED,0,0,0,0,NULL,NULL,instance,NULL);if(!hostWindow)return;
    memset(&hostIcon,0,sizeof(hostIcon));hostIcon.cbSize=sizeof(hostIcon);hostIcon.hWnd=hostWindow;hostIcon.uID=1;hostIcon.uFlags=NIF_MESSAGE|NIF_ICON|NIF_TIP;hostIcon.uCallbackMessage=WM_APP+1;hostIcon.hIcon=cls.hIcon;lstrcpyW(hostIcon.szTip,L"WorkBuddy Proxy");Shell_NotifyIconW(NIM_ADD,&hostIcon);
    SetTimer(hostWindow,1,500,NULL);wb_frontend_ready();wb_gpui_open();MSG message;while(GetMessageW(&message,NULL,0,0)>0){TranslateMessage(&message);DispatchMessageW(&message);}
}
void wb_gpui_host_quit(void){if(hostWindow)PostMessageW(hostWindow,WM_APP+4,0,0);}
struct ShowTarget { DWORD pid; HWND window; };
static BOOL CALLBACK findHost(HWND window,LPARAM data){
    struct ShowTarget *target=(struct ShowTarget *)data;DWORD pid=0;GetWindowThreadProcessId(window,&pid);
    wchar_t name[80];GetClassNameW(window,name,80);
    if(pid==target->pid&&lstrcmpW(name,L"WorkBuddyProxyWindow")==0){target->window=window;return FALSE;}
    return TRUE;
}
int wb_platform_show_process(unsigned int pid){
    struct ShowTarget target={pid,NULL};EnumWindows(findHost,(LPARAM)&target);
    if(!target.window)return 0;AllowSetForegroundWindow(pid);return PostMessageW(target.window,WM_APP+3,0,0)!=0;
}
void wb_platform_error(const char *message){
    int count=MultiByteToWideChar(CP_UTF8,0,message,-1,NULL,0);
    wchar_t *wide=(wchar_t *)HeapAlloc(GetProcessHeap(),0,(size_t)count*sizeof(wchar_t));
    if(wide){MultiByteToWideChar(CP_UTF8,0,message,-1,wide,count);MessageBoxW(NULL,wide,L"WorkBuddy Proxy",MB_OK|MB_ICONERROR);HeapFree(GetProcessHeap(),0,wide);}
}
