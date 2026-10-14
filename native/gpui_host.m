#import <AppKit/AppKit.h>
#include <unistd.h>
#include <string.h>
#include "icon_png.h"
extern void wb_gpui_open(void);
extern void wb_gpui_stop(void);
extern int wb_gpui_exit_requested(void);
extern void wb_frontend_ready(void);
extern void wb_frontend_closing(void);
extern char *wb_command(const char *);
extern char *wb_value(const char *);
extern void wb_free(char *);
@interface WBGPUIHost:NSObject<NSApplicationDelegate>
@property NSStatusItem *item;
@property NSTimer *timer;
@end
static WBGPUIHost *hostDelegate;
@implementation WBGPUIHost
- (void)applicationDidFinishLaunching:(NSNotification *)note{
    (void)note;char *lang=wb_value("language");BOOL zh=strcmp(lang,"zh")==0||(strcmp(lang,"auto")==0&&[NSLocale.preferredLanguages.firstObject hasPrefix:@"zh"]);wb_free(lang);
    self.item=[[NSStatusBar systemStatusBar]statusItemWithLength:NSSquareStatusItemLength];NSImage *image=[[NSImage alloc]initWithData:[NSData dataWithBytes:WB_ICON_PNG length:sizeof(WB_ICON_PNG)]];image.size=NSMakeSize(20,20);self.item.button.image=image;self.item.button.toolTip=@"WorkBuddy Proxy · GPUI";
    NSMenu *menu=[NSMenu new];for(int i=0;i<2;i++){NSMenuItem *entry=[[NSMenuItem alloc]initWithTitle:i?(zh?@"退出":@"Quit"):(zh?@"设置":@"Settings") action:i?@selector(quit):@selector(show) keyEquivalent:@""];entry.target=self;[menu addItem:entry];}self.item.menu=menu;
    [[NSDistributedNotificationCenter defaultCenter]addObserver:self selector:@selector(wake:) name:@"io.github.gloryhui.workbuddy-proxy.show" object:nil];self.timer=[NSTimer scheduledTimerWithTimeInterval:0.5 target:self selector:@selector(tick) userInfo:nil repeats:YES];wb_frontend_ready();wb_gpui_open();
}
- (void)show{wb_gpui_open();}
- (void)wake:(NSNotification *)note{if([note.object isEqualToString:[NSString stringWithFormat:@"%d",getpid()]])[self show];}
- (void)tick{if(wb_gpui_exit_requested())[self quit];}
- (BOOL)applicationShouldHandleReopen:(NSApplication *)app hasVisibleWindows:(BOOL)visible{(void)app;(void)visible;[self show];return NO;}
- (void)quit{wb_gpui_stop();wb_free(wb_command("quit"));[self.timer invalidate];[[NSStatusBar systemStatusBar]removeStatusItem:self.item];[[NSDistributedNotificationCenter defaultCenter]removeObserver:self];wb_frontend_closing();[NSApp stop:nil];[NSApp postEvent:[NSEvent otherEventWithType:NSEventTypeApplicationDefined location:NSZeroPoint modifierFlags:0 timestamp:0 windowNumber:0 context:nil subtype:0 data1:0 data2:0] atStart:YES];}
@end
void wb_gpui_host_run(void){@autoreleasepool{NSApplication *app=[NSApplication sharedApplication];[app setActivationPolicy:NSApplicationActivationPolicyAccessory];hostDelegate=[WBGPUIHost new];app.delegate=hostDelegate;[app run];hostDelegate=nil;}}
void wb_gpui_host_quit(void){dispatch_async(dispatch_get_main_queue(),^{if(hostDelegate)[hostDelegate quit];});}
