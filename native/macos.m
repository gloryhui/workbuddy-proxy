#import <AppKit/AppKit.h>
#include <stdlib.h>
#include <unistd.h>
extern char *wb_value(const char *name);
extern char *wb_save(const char *key,const char *model,const char *upstream,const char *effort);
extern char *wb_command(const char *name);
extern void wb_free(char *value);

static NSString *value(const char *name){char *p=wb_value(name);NSString *s=p?[NSString stringWithUTF8String:p]:@"";wb_free(p);return s?:@"";}
static NSString *take(char *p){NSString *s=p?[NSString stringWithUTF8String:p]:@"Internal error";wb_free(p);return s?:@"Internal error";}
@interface ProxyUI:NSObject<NSApplicationDelegate,NSWindowDelegate>
@property NSWindow *window;
@property NSStatusItem *item;
@property NSSecureTextField *key;
@property NSTextField *model;
@property NSTextField *upstream;
@property NSPopUpButton *effort;
@property NSTextField *status;
@property NSMutableArray<NSButton *> *buttons;
@end
static ProxyUI *ui;
@implementation ProxyUI
- (NSTextField *)label:(NSString *)text frame:(NSRect)frame{
    NSTextField *label=[NSTextField labelWithString:text];label.frame=frame;label.font=[NSFont systemFontOfSize:12];[self.window.contentView addSubview:label];return label;
}
- (NSButton *)button:(NSString *)title action:(SEL)action x:(CGFloat)x y:(CGFloat)y width:(CGFloat)width{
    NSButton *b=[NSButton buttonWithTitle:title target:self action:action];b.frame=NSMakeRect(x,y,width,32);[self.window.contentView addSubview:b];[self.buttons addObject:b];return b;
}
- (void)applicationDidFinishLaunching:(NSNotification *)notification{
    (void)notification;self.buttons=[NSMutableArray array];
    self.item=[[NSStatusBar systemStatusBar] statusItemWithLength:NSSquareStatusItemLength];
    self.item.button.image=[NSImage imageWithSystemSymbolName:@"arrow.left.arrow.right" accessibilityDescription:@"WorkBuddy Proxy"];
    NSMenu *menu=[NSMenu new];NSMenuItem *settings=[[NSMenuItem alloc]initWithTitle:@"设置 / Settings" action:@selector(show) keyEquivalent:@""];settings.target=self;[menu addItem:settings];
    NSMenuItem *quit=[[NSMenuItem alloc]initWithTitle:@"退出 / Quit" action:@selector(quit) keyEquivalent:@"q"];quit.target=self;[menu addItem:quit];self.item.menu=menu;
    self.window=[[NSWindow alloc]initWithContentRect:NSMakeRect(0,0,520,520) styleMask:NSWindowStyleMaskTitled|NSWindowStyleMaskClosable|NSWindowStyleMaskMiniaturizable backing:NSBackingStoreBuffered defer:NO];
    self.window.title=@"WorkBuddy Proxy";self.window.delegate=self;self.window.releasedWhenClosed=NO;
    NSTextField *heading=[self label:@"WorkBuddy Proxy" frame:NSMakeRect(24,472,470,28)];heading.font=[NSFont boldSystemFontOfSize:20];
    [self label:@"轻量原生转发 · Native streaming proxy" frame:NSMakeRect(24,447,470,22)];
    [self label:@"上游地址 / Upstream URL" frame:NSMakeRect(24,415,470,20)];
    self.upstream=[[NSTextField alloc]initWithFrame:NSMakeRect(24,383,472,26)];self.upstream.stringValue=value("upstream");[self.window.contentView addSubview:self.upstream];
    [self label:@"API Key（留空保留已保存值 / blank keeps saved key）" frame:NSMakeRect(24,350,472,20)];
    self.key=[[NSSecureTextField alloc]initWithFrame:NSMakeRect(24,318,472,26)];[self.window.contentView addSubview:self.key];
    [self label:@"模型名称 / Model ID" frame:NSMakeRect(24,285,472,20)];
    self.model=[[NSTextField alloc]initWithFrame:NSMakeRect(24,253,472,26)];self.model.stringValue=value("model");[self.window.contentView addSubview:self.model];
    [self label:@"思考强度 / Thinking effort" frame:NSMakeRect(24,218,472,20)];
    self.effort=[[NSPopUpButton alloc]initWithFrame:NSMakeRect(24,181,472,30) pullsDown:NO];[self.effort addItemsWithTitles:@[@"跟随客户端 / Default",@"关闭思考 / Off",@"低 / Low",@"高 / High",@"最高 / Max"]];
    NSArray *efforts=@[@"default",@"none",@"low",@"high",@"max"];NSUInteger index=[efforts indexOfObject:value("reasoning_effort")];[self.effort selectItemAtIndex:index==NSNotFound?0:index];[self.window.contentView addSubview:self.effort];
    [self button:@"保存 / Save" action:@selector(save) x:24 y:133 width:105];
    [self button:@"测试 / Test" action:@selector(test) x:139 y:133 width:105];
    [self button:@"复制配置 / Copy" action:@selector(copyConfiguration) x:254 y:133 width:145];
    [self button:@"配置 WorkBuddy" action:@selector(install) x:24 y:93 width:180];
    [self button:@"恢复原配置 / Restore" action:@selector(restore) x:214 y:93 width:185];
    self.status=[self label:@"" frame:NSMakeRect(24,14,472,65)];self.status.lineBreakMode=NSLineBreakByWordWrapping;self.status.maximumNumberOfLines=3;
    [self.window center];[self show];
}
- (BOOL)windowShouldClose:(NSWindow *)sender{(void)sender;[NSApp stop:nil];NSEvent *event=[NSEvent otherEventWithType:NSEventTypeApplicationDefined location:NSZeroPoint modifierFlags:0 timestamp:0 windowNumber:0 context:nil subtype:0 data1:0 data2:0];[NSApp postEvent:event atStart:YES];return YES;}
- (void)show{self.status.stringValue=[NSString stringWithFormat:@"%@\n%@",value("url"),value("status")];[self.window makeKeyAndOrderFront:nil];[NSApp activateIgnoringOtherApps:YES];}
- (void)save{NSString *result=take(wb_save(self.key.stringValue.UTF8String,self.model.stringValue.UTF8String,self.upstream.stringValue.UTF8String,[@[@"default",@"none",@"low",@"high",@"max"][self.effort.indexOfSelectedItem] UTF8String]));self.status.stringValue=result;if([result hasPrefix:@"OK "]){self.key.stringValue=@"";}}
- (void)execute:(const char *)command{
    for(NSButton *b in self.buttons)b.enabled=NO;self.status.stringValue=@"正在处理 / Working…";
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED,0),^{
        NSString *result=take(wb_command(command));dispatch_async(dispatch_get_main_queue(),^{for(NSButton *b in self.buttons)b.enabled=YES;self.status.stringValue=result;});
    });
}
- (void)test{[self execute:"test"];}
- (void)install{[self execute:"install"];}
- (void)restore{[self execute:"restore"];}
- (void)copyConfiguration{NSString *result=take(wb_command("copy"));if([result hasPrefix:@"OK "]){NSPasteboard *p=[NSPasteboard generalPasteboard];[p clearContents];[p setString:[result substringFromIndex:3] forType:NSPasteboardTypeString];self.status.stringValue=@"配置已复制 / Configuration copied";}else{self.status.stringValue=result;}}
- (void)quit{wb_free(wb_command("quit"));[NSApp stop:nil];NSEvent *event=[NSEvent otherEventWithType:NSEventTypeApplicationDefined location:NSZeroPoint modifierFlags:0 timestamp:0 windowNumber:0 context:nil subtype:0 data1:0 data2:0];[NSApp postEvent:event atStart:YES];}
@end
void wb_native_run(void){@autoreleasepool{[NSApplication sharedApplication];[NSApp setActivationPolicy:NSApplicationActivationPolicyAccessory];ui=[ProxyUI new];NSApp.delegate=ui;[NSApp run];ui=nil;}}
void wb_native_show(void){if(ui){dispatch_async(dispatch_get_main_queue(),^{[ui show];});}else{for(NSRunningApplication *app in [NSRunningApplication runningApplicationsWithBundleIdentifier:@"io.github.gloryhui.workbuddy-proxy"]){if(app.processIdentifier!=getpid()){[app activateWithOptions:NSApplicationActivateIgnoringOtherApps];}}}}
void wb_native_quit(void){dispatch_async(dispatch_get_main_queue(),^{[ui quit];});}
void wb_native_error(const char *message){@autoreleasepool{[NSApplication sharedApplication];NSAlert *alert=[NSAlert new];alert.messageText=@"WorkBuddy Proxy";alert.informativeText=[NSString stringWithUTF8String:message]?:@"Startup failed";[alert runModal];}}
