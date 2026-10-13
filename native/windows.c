#define UNICODE
#define _UNICODE
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <shellapi.h>
#include <stdlib.h>
#include <string.h>
extern char *wb_value(const char *name);
extern char *wb_save(const char *key,const char *model,const char *upstream);
extern char *wb_command(const char *name);
extern void wb_free(char *value);

#define WM_TRAY (WM_APP+1)
#define WM_RESULT (WM_APP+2)
#define WM_SHOW (WM_APP+3)
#define WM_STOP (WM_APP+4)
static HWND window,keyEdit,modelEdit,urlEdit,statusLabel,buttons[5];
static HFONT font;
static BOOL busy=FALSE;
static NOTIFYICONDATAW tray;

static wchar_t *wide(const char *s){int n=MultiByteToWideChar(CP_UTF8,0,s,-1,NULL,0);if(n<=0)n=1;wchar_t *v=(wchar_t *)calloc((size_t)n,sizeof(wchar_t));if(v)MultiByteToWideChar(CP_UTF8,0,s,-1,v,n);return v;}
static void text(HWND h,const char *s){wchar_t *v=wide(s);if(v){SetWindowTextW(h,v);free(v);}}
static char *editText(HWND h){int n=GetWindowTextLengthW(h)+1;wchar_t *w=(wchar_t *)calloc((size_t)n,sizeof(wchar_t));if(!w)return NULL;GetWindowTextW(h,w,n);int size=WideCharToMultiByte(CP_UTF8,0,w,-1,NULL,0,NULL,NULL);char *v=(char *)calloc((size_t)(size>0?size:1),1);if(v)WideCharToMultiByte(CP_UTF8,0,w,-1,v,size,NULL,NULL);free(w);return v;}
static HWND control(const wchar_t *cls,const wchar_t *title,DWORD style,int x,int y,int width,int height,int id){HWND h=CreateWindowExW(0,cls,title,WS_CHILD|WS_VISIBLE|style,x,y,width,height,window,(HMENU)(INT_PTR)id,GetModuleHandleW(NULL),NULL);SendMessageW(h,WM_SETFONT,(WPARAM)font,TRUE);return h;}
static void status(const char *s){text(statusLabel,s);}
static void refresh(void){char *m=wb_value("model"),*u=wb_value("upstream"),*state=wb_value("status");text(modelEdit,m);text(urlEdit,u);status(state);wb_free(m);wb_free(u);wb_free(state);}
static void show(void){ShowWindow(window,SW_SHOW);SetForegroundWindow(window);refresh();}
static void setBusy(BOOL value){busy=value;for(int i=0;i<5;i++)EnableWindow(buttons[i],!value);}
static DWORD WINAPI work(LPVOID command){char *result=wb_command((const char *)command);if(!PostMessageW(window,WM_RESULT,0,(LPARAM)result))wb_free(result);return 0;}
static void execute(const char *command){if(busy)return;setBusy(TRUE);status("Working / 正在处理...");HANDLE thread=CreateThread(NULL,256*1024,work,(LPVOID)command,0,NULL);if(thread)CloseHandle(thread);else{setBusy(FALSE);status("Cannot start worker / 无法启动工作线程");}}
static void copyConfiguration(void){char *value=wb_command("copy");if(strncmp(value,"OK ",3)==0){wchar_t *w=wide(value+3);if(w){SIZE_T size=(wcslen(w)+1)*sizeof(wchar_t);HGLOBAL data=GlobalAlloc(GMEM_MOVEABLE,size);if(data){void *p=GlobalLock(data);if(p){memcpy(p,w,size);GlobalUnlock(data);if(OpenClipboard(window)){EmptyClipboard();if(!SetClipboardData(CF_UNICODETEXT,data))GlobalFree(data);CloseClipboard();status("Copied / 配置已复制");}else{GlobalFree(data);status("Clipboard is busy / 剪贴板忙");}}else{GlobalFree(data);}}free(w);}}else status(value);wb_free(value);}
static LRESULT CALLBACK procedure(HWND h,UINT msg,WPARAM w,LPARAM l){
    switch(msg){
        case WM_CREATE:{
            window=h;font=CreateFontW(-16,0,0,0,FW_NORMAL,FALSE,FALSE,FALSE,DEFAULT_CHARSET,OUT_DEFAULT_PRECIS,CLIP_DEFAULT_PRECIS,CLEARTYPE_QUALITY,DEFAULT_PITCH,L"Segoe UI");
            control(L"STATIC",L"WorkBuddy Proxy",0,20,16,470,28,0);
            control(L"STATIC",L"上游地址 / Upstream URL",0,20,55,470,22,0);urlEdit=control(L"EDIT",L"",WS_BORDER|ES_AUTOHSCROLL|WS_TABSTOP,20,80,470,28,101);SendMessageW(urlEdit,EM_SETLIMITTEXT,2048,0);
            control(L"STATIC",L"API Key（留空保留 / blank keeps saved key）",0,20,120,470,22,0);keyEdit=control(L"EDIT",L"",WS_BORDER|ES_AUTOHSCROLL|ES_PASSWORD|WS_TABSTOP,20,145,470,28,102);SendMessageW(keyEdit,EM_SETLIMITTEXT,4096,0);
            control(L"STATIC",L"模型名称 / Model ID",0,20,185,470,22,0);modelEdit=control(L"EDIT",L"",WS_BORDER|ES_AUTOHSCROLL|WS_TABSTOP,20,210,470,28,103);SendMessageW(modelEdit,EM_SETLIMITTEXT,160,0);
            buttons[0]=control(L"BUTTON",L"保存 / Save",WS_TABSTOP,20,255,130,32,201);buttons[1]=control(L"BUTTON",L"测试 / Test",WS_TABSTOP,165,255,130,32,202);buttons[2]=control(L"BUTTON",L"复制配置 / Copy",WS_TABSTOP,310,255,180,32,203);
            buttons[3]=control(L"BUTTON",L"配置 WorkBuddy",WS_TABSTOP,20,300,225,32,204);buttons[4]=control(L"BUTTON",L"恢复配置 / Restore",WS_TABSTOP,260,300,230,32,205);
            statusLabel=control(L"STATIC",L"",0,20,350,470,60,0);refresh();
            memset(&tray,0,sizeof(tray));tray.cbSize=sizeof(tray);tray.hWnd=h;tray.uID=1;tray.uFlags=NIF_MESSAGE|NIF_ICON|NIF_TIP;tray.uCallbackMessage=WM_TRAY;tray.hIcon=LoadIconW(NULL,IDI_APPLICATION);lstrcpyW(tray.szTip,L"WorkBuddy Proxy");Shell_NotifyIconW(NIM_ADD,&tray);return 0;
        }
        case WM_COMMAND:switch(LOWORD(w)){
            case 201:{char *key=editText(keyEdit),*model=editText(modelEdit),*url=editText(urlEdit);char *r=wb_save(key,model,url);status(r);if(strncmp(r,"OK ",3)==0)SetWindowTextW(keyEdit,L"");free(key);free(model);free(url);wb_free(r);return 0;}
            case 202:execute("test");return 0;case 203:copyConfiguration();return 0;case 204:execute("install");return 0;case 205:execute("restore");return 0;
            case 301:show();return 0;case 302:wb_free(wb_command("quit"));DestroyWindow(h);return 0;
        }break;
        case WM_RESULT:{char *r=(char *)l;status(r);wb_free(r);setBusy(FALSE);return 0;}
        case WM_SHOW:show();return 0;
        case WM_STOP:DestroyWindow(h);return 0;
        case WM_CLOSE:ShowWindow(h,SW_HIDE);return 0;
        case WM_TRAY:
            if(l==WM_LBUTTONUP||l==WM_LBUTTONDBLCLK){show();return 0;}
            if(l==WM_RBUTTONUP){POINT p;GetCursorPos(&p);HMENU menu=CreatePopupMenu();AppendMenuW(menu,MF_STRING,301,L"设置 / Settings");AppendMenuW(menu,MF_STRING,302,L"退出 / Quit");SetForegroundWindow(h);TrackPopupMenu(menu,TPM_RIGHTBUTTON,p.x,p.y,0,h,NULL);DestroyMenu(menu);return 0;}break;
        case WM_DESTROY:Shell_NotifyIconW(NIM_DELETE,&tray);if(font)DeleteObject(font);window=NULL;PostQuitMessage(0);return 0;
    }return DefWindowProcW(h,msg,w,l);
}
void wb_native_run(void){
    SetProcessDPIAware();HINSTANCE instance=GetModuleHandleW(NULL);WNDCLASSW cls;memset(&cls,0,sizeof(cls));cls.lpfnWndProc=procedure;cls.hInstance=instance;cls.lpszClassName=L"WorkBuddyProxyWindow";cls.hCursor=LoadCursorW(NULL,IDC_ARROW);cls.hbrBackground=(HBRUSH)(COLOR_WINDOW+1);cls.hIcon=LoadIconW(NULL,IDI_APPLICATION);
    if(!RegisterClassW(&cls)){MessageBoxW(NULL,L"Cannot register native window",L"WorkBuddy Proxy",MB_ICONERROR);return;}
    HWND h=CreateWindowExW(0,cls.lpszClassName,L"WorkBuddy Proxy",WS_OVERLAPPED|WS_CAPTION|WS_SYSMENU|WS_MINIMIZEBOX,CW_USEDEFAULT,CW_USEDEFAULT,530,460,NULL,NULL,instance,NULL);
    if(!h){MessageBoxW(NULL,L"Cannot create settings window",L"WorkBuddy Proxy",MB_ICONERROR);return;}ShowWindow(h,SW_SHOW);UpdateWindow(h);
    MSG message;while(GetMessageW(&message,NULL,0,0)>0){if(!IsDialogMessageW(h,&message)){TranslateMessage(&message);DispatchMessageW(&message);}}
}
void wb_native_show(void){if(window)PostMessageW(window,WM_SHOW,0,0);}
void wb_native_quit(void){if(window)PostMessageW(window,WM_STOP,0,0);}
void wb_native_error(const char *message){wchar_t *w=wide(message);MessageBoxW(NULL,w?w:L"Startup failed",L"WorkBuddy Proxy",MB_ICONERROR);free(w);}
