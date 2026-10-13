#define UNICODE
#define _UNICODE
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <shellapi.h>
#include <stdlib.h>
#include <string.h>
extern char *wb_value(const char *name);
extern char *wb_save(const char *name,const char *key,const char *model,const char *upstream,const char *effort);
extern void wb_select(int index);
extern char *wb_command(const char *name);
extern void wb_free(char *value);

#define WM_TRAY (WM_APP+1)
#define WM_RESULT (WM_APP+2)
#define WM_SHOW (WM_APP+3)
#define WM_STOP (WM_APP+4)
static HWND window,connectionList,nameEdit,keyEdit,modelEdit,urlEdit,effortCombo,statusLabel,buttons[8];
static HFONT font;
static HICON appIcon,smallIcon;
static BOOL busy=FALSE;
static NOTIFYICONDATAW tray;
static BOOL trayAvailable=FALSE;
static UINT taskbarCreated;
static const char *efforts[]={"default","none","low","high","max","keep"};

static wchar_t *wide(const char *s){int n=MultiByteToWideChar(CP_UTF8,0,s,-1,NULL,0);if(n<=0)n=1;wchar_t *v=(wchar_t *)calloc((size_t)n,sizeof(wchar_t));if(v)MultiByteToWideChar(CP_UTF8,0,s,-1,v,n);return v;}
static void text(HWND h,const char *s){wchar_t *v=wide(s);if(v){SetWindowTextW(h,v);free(v);}}
static char *editText(HWND h){int n=GetWindowTextLengthW(h)+1;wchar_t *w=(wchar_t *)calloc((size_t)n,sizeof(wchar_t));if(!w)return NULL;GetWindowTextW(h,w,n);int size=WideCharToMultiByte(CP_UTF8,0,w,-1,NULL,0,NULL,NULL);char *v=(char *)calloc((size_t)(size>0?size:1),1);if(v)WideCharToMultiByte(CP_UTF8,0,w,-1,v,size,NULL,NULL);free(w);return v;}
static HWND control(const wchar_t *cls,const wchar_t *title,DWORD style,int x,int y,int width,int height,int id){HWND h=CreateWindowExW(0,cls,title,WS_CHILD|WS_VISIBLE|style,x,y,width,height,window,(HMENU)(INT_PTR)id,GetModuleHandleW(NULL),NULL);SendMessageW(h,WM_SETFONT,(WPARAM)font,TRUE);return h;}
static void status(const char *s){text(statusLabel,s);}
static void fields(void){const char *names[]={"name","model","upstream"};HWND controls[]={nameEdit,modelEdit,urlEdit};for(int i=0;i<3;i++){char *v=wb_value(names[i]);if(controls[i]==modelEdit){size_t n=strlen(v);char *lines=(char *)calloc(n*2+1,1);if(lines){size_t j=0;for(size_t k=0;k<n;k++){if(v[k]=='\n')lines[j++]='\r';lines[j++]=v[k];}text(modelEdit,lines);free(lines);}}else text(controls[i],v);wb_free(v);}SetWindowTextW(keyEdit,L"");char *e=wb_value("reasoning_effort");int index=0;for(int i=0;i<6;i++)if(strcmp(e,efforts[i])==0)index=i;SendMessageW(effortCombo,CB_SETCURSEL,index,0);wb_free(e);}
static void refresh(void){char *list=wb_value("connections");SendMessageW(connectionList,LB_RESETCONTENT,0,0);char *row=list;while(*row){char *end=strchr(row,'\n');if(end)*end=0;wchar_t *w=wide(row);if(w){SendMessageW(connectionList,LB_ADDSTRING,0,(LPARAM)w);free(w);}if(!end)break;row=end+1;}char *selected=wb_value("selected");SendMessageW(connectionList,LB_SETCURSEL,atoi(selected),0);wb_free(selected);wb_free(list);fields();}
static void show(void){ShowWindow(window,SW_RESTORE);SetForegroundWindow(window);refresh();}
static void setBusy(BOOL value){busy=value;for(int i=0;i<8;i++)EnableWindow(buttons[i],!value);EnableWindow(connectionList,!value);}
static DWORD WINAPI work(LPVOID command){char *result=wb_command((const char *)command);if(!PostMessageW(window,WM_RESULT,0,(LPARAM)result))wb_free(result);return 0;}
static void execute(const char *command){if(busy)return;setBusy(TRUE);status("Working / 正在处理...");HANDLE thread=CreateThread(NULL,256*1024,work,(LPVOID)command,0,NULL);if(thread)CloseHandle(thread);else{setBusy(FALSE);status("Cannot start worker / 无法启动工作线程");}}
static void copyConfiguration(void){char *value=wb_command("copy");if(strncmp(value,"OK ",3)==0){wchar_t *w=wide(value+3);if(w){SIZE_T size=(wcslen(w)+1)*sizeof(wchar_t);HGLOBAL data=GlobalAlloc(GMEM_MOVEABLE,size);if(data){void *p=GlobalLock(data);if(p){memcpy(p,w,size);GlobalUnlock(data);if(OpenClipboard(window)){EmptyClipboard();if(!SetClipboardData(CF_UNICODETEXT,data))GlobalFree(data);CloseClipboard();status("Copied / 配置已复制");}else{GlobalFree(data);status("Clipboard is busy / 剪贴板忙");}}else{GlobalFree(data);}}free(w);}}else status(value);wb_free(value);}
static LRESULT CALLBACK procedure(HWND h,UINT msg,WPARAM w,LPARAM l){
    if(taskbarCreated&&msg==taskbarCreated){trayAvailable=Shell_NotifyIconW(NIM_ADD,&tray);if(!trayAvailable)show();return 0;}
    switch(msg){
        case WM_CREATE:{
            window=h;font=CreateFontW(-16,0,0,0,FW_NORMAL,FALSE,FALSE,FALSE,DEFAULT_CHARSET,OUT_DEFAULT_PRECIS,CLIP_DEFAULT_PRECIS,CLEARTYPE_QUALITY,DEFAULT_PITCH,L"Segoe UI");
            HWND logo=control(L"STATIC",L"",SS_ICON,20,12,32,32,108);SendMessageW(logo,STM_SETICON,(WPARAM)appIcon,0);
            control(L"STATIC",L"WorkBuddy Proxy",0,62,16,648,28,0);
            control(L"STATIC",L"供应商 / Providers",0,20,48,230,22,0);
            connectionList=control(L"LISTBOX",L"",WS_BORDER|LBS_NOTIFY|WS_VSCROLL|WS_TABSTOP,20,75,230,445,107);
            buttons[5]=control(L"BUTTON",L"新增",WS_TABSTOP,20,530,70,32,206);buttons[6]=control(L"BUTTON",L"复制",WS_TABSTOP,100,530,70,32,207);buttons[7]=control(L"BUTTON",L"删除",WS_TABSTOP,180,530,70,32,208);
            control(L"STATIC",L"供应商名称 / Provider name",0,270,48,440,22,0);nameEdit=control(L"EDIT",L"",WS_BORDER|ES_AUTOHSCROLL|WS_TABSTOP,270,73,440,28,105);SendMessageW(nameEdit,EM_SETLIMITTEXT,80,0);
            control(L"STATIC",L"OpenAI 兼容地址 / Base URL",0,270,113,440,22,0);urlEdit=control(L"EDIT",L"",WS_BORDER|ES_AUTOHSCROLL|WS_TABSTOP,270,138,440,28,101);SendMessageW(urlEdit,EM_SETLIMITTEXT,2048,0);
            control(L"STATIC",L"API Key（留空保留已保存值）",0,270,178,440,22,0);keyEdit=control(L"EDIT",L"",WS_BORDER|ES_AUTOHSCROLL|ES_PASSWORD|WS_TABSTOP,270,203,440,28,102);SendMessageW(keyEdit,EM_SETLIMITTEXT,4096,0);
            control(L"STATIC",L"模型列表：每行 ID，可填写 ID | 显示名称",0,270,243,440,22,0);modelEdit=control(L"EDIT",L"",WS_BORDER|ES_MULTILINE|ES_AUTOVSCROLL|ES_WANTRETURN|WS_VSCROLL|WS_TABSTOP,270,268,440,110,103);SendMessageW(modelEdit,EM_SETLIMITTEXT,60000,0);
            control(L"STATIC",L"思考强度（默认透明透传）",0,270,388,440,22,0);
            effortCombo=control(L"COMBOBOX",L"",CBS_DROPDOWNLIST|WS_VSCROLL|WS_TABSTOP,270,413,440,200,104);
            const wchar_t *labels[]={L"跟随 WorkBuddy / Default",L"关闭思考 / Off",L"低 / Low",L"高 / High",L"最高 / Max",L"保留各模型设置 / Keep"};for(int i=0;i<6;i++)SendMessageW(effortCombo,CB_ADDSTRING,0,(LPARAM)labels[i]);
            buttons[0]=control(L"BUTTON",L"保存 / Save",WS_TABSTOP,270,450,130,32,201);buttons[1]=control(L"BUTTON",L"测试首项 / Test",WS_TABSTOP,415,450,130,32,202);buttons[2]=control(L"BUTTON",L"复制配置",WS_TABSTOP,560,450,150,32,203);
            buttons[3]=control(L"BUTTON",L"同步 WorkBuddy 列表",WS_TABSTOP,270,495,215,32,204);buttons[4]=control(L"BUTTON",L"恢复原配置 / Restore",WS_TABSTOP,500,495,210,32,205);
            statusLabel=control(L"STATIC",L"",0,270,545,440,65,0);refresh();char *state=wb_value("status");status(state);wb_free(state);
            memset(&tray,0,sizeof(tray));tray.cbSize=sizeof(tray);tray.hWnd=h;tray.uID=1;tray.uFlags=NIF_MESSAGE|NIF_ICON|NIF_TIP;tray.uCallbackMessage=WM_TRAY;tray.hIcon=smallIcon;lstrcpyW(tray.szTip,L"WorkBuddy Proxy");trayAvailable=Shell_NotifyIconW(NIM_ADD,&tray);return 0;
        }
        case WM_COMMAND:switch(LOWORD(w)){
            case 107:if(HIWORD(w)==LBN_SELCHANGE){wb_select((int)SendMessageW(connectionList,LB_GETCURSEL,0,0));fields();}return 0;
            case 201:{char *name=editText(nameEdit),*key=editText(keyEdit),*model=editText(modelEdit),*url=editText(urlEdit);LRESULT i=SendMessageW(effortCombo,CB_GETCURSEL,0,0);char *r=wb_save(name,key,model,url,efforts[i>=0&&i<6?i:0]);if(strncmp(r,"OK ",3)==0)refresh();status(r);free(name);free(key);free(model);free(url);wb_free(r);return 0;}
            case 202:execute("test");return 0;case 203:copyConfiguration();return 0;case 204:execute("install");return 0;case 205:execute("restore");return 0;
            case 206:case 207:case 208:{char *r=wb_command(LOWORD(w)==206?"add":LOWORD(w)==207?"duplicate":"delete");if(strncmp(r,"OK ",3)==0)refresh();status(r);wb_free(r);return 0;}
            case 301:show();return 0;case 302:wb_free(wb_command("quit"));DestroyWindow(h);return 0;
        }break;
        case WM_RESULT:{char *r=(char *)l;status(r);wb_free(r);setBusy(FALSE);return 0;}
        case WM_SHOW:show();return 0;
        case WM_STOP:DestroyWindow(h);return 0;
        case WM_CLOSE:ShowWindow(h,trayAvailable?SW_HIDE:SW_MINIMIZE);return 0;
        case WM_SIZE:if(w==SIZE_MINIMIZED&&trayAvailable){ShowWindow(h,SW_HIDE);return 0;}break;
        case WM_TRAY:
            if(l==WM_LBUTTONUP||l==WM_LBUTTONDBLCLK){show();return 0;}
            if(l==WM_RBUTTONUP){POINT p;GetCursorPos(&p);HMENU menu=CreatePopupMenu();AppendMenuW(menu,MF_STRING,301,L"设置 / Settings");AppendMenuW(menu,MF_STRING,302,L"退出 / Quit");SetForegroundWindow(h);TrackPopupMenu(menu,TPM_RIGHTBUTTON,p.x,p.y,0,h,NULL);DestroyMenu(menu);return 0;}break;
        case WM_DESTROY:Shell_NotifyIconW(NIM_DELETE,&tray);if(font)DeleteObject(font);window=NULL;PostQuitMessage(0);return 0;
    }return DefWindowProcW(h,msg,w,l);
}
void wb_native_run(void){
    SetProcessDPIAware();taskbarCreated=RegisterWindowMessageW(L"TaskbarCreated");HINSTANCE instance=GetModuleHandleW(NULL);appIcon=(HICON)LoadImageW(instance,MAKEINTRESOURCEW(1),IMAGE_ICON,GetSystemMetrics(SM_CXICON),GetSystemMetrics(SM_CYICON),LR_SHARED);smallIcon=(HICON)LoadImageW(instance,MAKEINTRESOURCEW(1),IMAGE_ICON,GetSystemMetrics(SM_CXSMICON),GetSystemMetrics(SM_CYSMICON),LR_SHARED);WNDCLASSW cls;memset(&cls,0,sizeof(cls));cls.lpfnWndProc=procedure;cls.hInstance=instance;cls.lpszClassName=L"WorkBuddyProxyWindow";cls.hCursor=LoadCursorW(NULL,IDC_ARROW);cls.hbrBackground=(HBRUSH)(COLOR_WINDOW+1);cls.hIcon=appIcon;
    if(!RegisterClassW(&cls)){MessageBoxW(NULL,L"Cannot register native window",L"WorkBuddy Proxy",MB_ICONERROR);return;}
    HWND h=CreateWindowExW(0,cls.lpszClassName,L"WorkBuddy Proxy",WS_OVERLAPPED|WS_CAPTION|WS_SYSMENU|WS_MINIMIZEBOX,CW_USEDEFAULT,CW_USEDEFAULT,750,660,NULL,NULL,instance,NULL);
    if(!h){MessageBoxW(NULL,L"Cannot create settings window",L"WorkBuddy Proxy",MB_ICONERROR);return;}SendMessageW(h,WM_SETICON,ICON_BIG,(LPARAM)appIcon);SendMessageW(h,WM_SETICON,ICON_SMALL,(LPARAM)smallIcon);ShowWindow(h,SW_SHOW);UpdateWindow(h);
    MSG message;while(GetMessageW(&message,NULL,0,0)>0){if(!IsDialogMessageW(h,&message)){TranslateMessage(&message);DispatchMessageW(&message);}}
}
void wb_native_show(void){HWND h=window?window:FindWindowW(L"WorkBuddyProxyWindow",NULL);if(h)PostMessageW(h,WM_SHOW,0,0);}
void wb_native_quit(void){if(window)PostMessageW(window,WM_STOP,0,0);}
void wb_native_error(const char *message){wchar_t *w=wide(message);MSGBOXPARAMSW box;memset(&box,0,sizeof(box));box.cbSize=sizeof(box);box.hInstance=GetModuleHandleW(NULL);box.lpszText=w?w:L"Startup failed";box.lpszCaption=L"WorkBuddy Proxy";box.dwStyle=MB_USERICON;box.lpszIcon=MAKEINTRESOURCEW(1);MessageBoxIndirectW(&box);free(w);}
