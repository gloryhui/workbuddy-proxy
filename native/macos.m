#import <AppKit/AppKit.h>
#include <stdlib.h>
#include <unistd.h>
extern char *wb_value(const char *name);
extern char *wb_save(const char *name,const char *provider,const char *key,const char *model,const char *upstream,const char *effort);
extern void wb_select(int index);
extern char *wb_command(const char *name);
extern void wb_free(char *value);
static NSString *take(char *p){NSString *s=p?[NSString stringWithUTF8String:p]:@"Internal error";wb_free(p);return s?:@"Internal error";}
static NSString *value(const char *name){return take(wb_value(name));}
@interface ProxyUI:NSObject<NSApplicationDelegate,NSWindowDelegate,NSTableViewDataSource,NSTableViewDelegate>
@property NSWindow *window;
@property NSStatusItem *item;
@property NSTableView *list;
@property NSArray<NSString *> *rows;
@property BOOL refreshing;
@property NSTextField *name;
@property NSTextField *provider;
@property NSSecureTextField *key;
@property NSTextField *model;
@property NSTextField *upstream;
@property NSPopUpButton *effort;
@property NSTextField *status;
@property NSMutableArray<NSButton *> *buttons;
@end
static ProxyUI *ui;
static void stopUI(void){[NSApp stop:nil];NSEvent *event=[NSEvent otherEventWithType:NSEventTypeApplicationDefined location:NSZeroPoint modifierFlags:0 timestamp:0 windowNumber:0 context:nil subtype:0 data1:0 data2:0];[NSApp postEvent:event atStart:YES];}
@implementation ProxyUI
- (NSTextField *)label:(NSString *)text frame:(NSRect)frame{NSTextField *label=[NSTextField labelWithString:text];label.frame=frame;label.font=[NSFont systemFontOfSize:12];[self.window.contentView addSubview:label];return label;}
- (NSTextField *)field:(NSString *)title y:(CGFloat)y{[self label:title frame:NSMakeRect(280,y+32,470,20)];NSTextField *f=[[NSTextField alloc]initWithFrame:NSMakeRect(280,y,470,26)];[self.window.contentView addSubview:f];return f;}
- (NSButton *)button:(NSString *)title action:(SEL)action x:(CGFloat)x y:(CGFloat)y width:(CGFloat)width{NSButton *b=[NSButton buttonWithTitle:title target:self action:action];b.frame=NSMakeRect(x,y,width,32);[self.window.contentView addSubview:b];[self.buttons addObject:b];return b;}
- (void)applicationDidFinishLaunching:(NSNotification *)notification{
    (void)notification;self.buttons=[NSMutableArray array];self.rows=@[];
    self.item=[[NSStatusBar systemStatusBar] statusItemWithLength:NSSquareStatusItemLength];self.item.button.image=[NSImage imageWithSystemSymbolName:@"arrow.left.arrow.right" accessibilityDescription:@"WorkBuddy Proxy"];
    NSMenu *menu=[NSMenu new];NSMenuItem *settings=[[NSMenuItem alloc]initWithTitle:@"设置 / Settings" action:@selector(show) keyEquivalent:@""];settings.target=self;[menu addItem:settings];NSMenuItem *quit=[[NSMenuItem alloc]initWithTitle:@"退出 / Quit" action:@selector(quit) keyEquivalent:@"q"];quit.target=self;[menu addItem:quit];self.item.menu=menu;
    self.window=[[NSWindow alloc]initWithContentRect:NSMakeRect(0,0,780,650) styleMask:NSWindowStyleMaskTitled|NSWindowStyleMaskClosable|NSWindowStyleMaskMiniaturizable backing:NSBackingStoreBuffered defer:NO];self.window.title=@"WorkBuddy Proxy";self.window.delegate=self;self.window.releasedWhenClosed=NO;
    NSTextField *heading=[self label:@"WorkBuddy Proxy" frame:NSMakeRect(20,602,730,28)];heading.font=[NSFont boldSystemFontOfSize:20];
    [self label:@"连接列表 / Connections" frame:NSMakeRect(20,569,240,20)];
    NSScrollView *scroll=[[NSScrollView alloc]initWithFrame:NSMakeRect(20,183,240,376)];scroll.hasVerticalScroller=YES;scroll.borderType=NSBezelBorder;
    self.list=[[NSTableView alloc]initWithFrame:scroll.bounds];NSTableColumn *column=[[NSTableColumn alloc]initWithIdentifier:@"connection"];column.width=230;[self.list addTableColumn:column];self.list.headerView=nil;self.list.rowHeight=30;self.list.dataSource=self;self.list.delegate=self;self.list.allowsEmptySelection=NO;scroll.documentView=self.list;[self.window.contentView addSubview:scroll];
    [self button:@"新增" action:@selector(add) x:20 y:133 width:74];[self button:@"复制" action:@selector(duplicate) x:100 y:133 width:74];[self button:@"删除" action:@selector(remove) x:180 y:133 width:74];
    self.name=[self field:@"列表显示名称 / Display name" y:537];self.provider=[self field:@"供应商 / Provider" y:472];self.upstream=[self field:@"上游地址 / Upstream URL" y:407];
    [self label:@"API Key（留空保留已保存值）" frame:NSMakeRect(280,374,470,20)];self.key=[[NSSecureTextField alloc]initWithFrame:NSMakeRect(280,342,470,26)];[self.window.contentView addSubview:self.key];
    self.model=[self field:@"上游真实模型 ID / Model ID" y:277];
    [self label:@"思考强度 / Thinking effort" frame:NSMakeRect(280,244,470,20)];self.effort=[[NSPopUpButton alloc]initWithFrame:NSMakeRect(280,207,470,30) pullsDown:NO];[self.effort addItemsWithTitles:@[@"跟随 WorkBuddy / Default",@"关闭思考 / Off",@"低 / Low",@"高 / High",@"最高 / Max"]];[self.window.contentView addSubview:self.effort];
    [self button:@"保存 / Save" action:@selector(save) x:280 y:161 width:120];[self button:@"测试此项 / Test" action:@selector(test) x:410 y:161 width:150];[self button:@"复制配置" action:@selector(copyConfiguration) x:570 y:161 width:180];
    [self button:@"同步 WorkBuddy 列表" action:@selector(install) x:280 y:121 width:230];[self button:@"恢复原配置 / Restore" action:@selector(restore) x:520 y:121 width:230];
    self.status=[self label:@"" frame:NSMakeRect(280,28,470,76)];self.status.lineBreakMode=NSLineBreakByWordWrapping;self.status.maximumNumberOfLines=4;
    [self.window center];[self show];
}
- (NSInteger)numberOfRowsInTableView:(NSTableView *)tableView{(void)tableView;return self.rows.count;}
- (NSView *)tableView:(NSTableView *)tableView viewForTableColumn:(NSTableColumn *)column row:(NSInteger)row{(void)tableView;(void)column;NSTextField *f=[NSTextField labelWithString:self.rows[row]];f.lineBreakMode=NSLineBreakByTruncatingTail;f.toolTip=self.rows[row];return f;}
- (void)tableViewSelectionDidChange:(NSNotification *)notification{(void)notification;if(!self.refreshing&&self.list.selectedRow>=0){wb_select((int)self.list.selectedRow);[self fields];}}
- (void)fields{self.name.stringValue=value("name");self.provider.stringValue=value("provider");self.upstream.stringValue=value("upstream");self.model.stringValue=value("model");self.key.stringValue=@"";NSArray *efforts=@[@"default",@"none",@"low",@"high",@"max"];NSUInteger i=[efforts indexOfObject:value("reasoning_effort")];[self.effort selectItemAtIndex:i==NSNotFound?0:i];}
- (void)refresh{self.refreshing=YES;NSString *rows=value("connections");self.rows=rows.length?[rows componentsSeparatedByString:@"\n"]:@[];[self.list reloadData];NSInteger index=[value("selected") integerValue];if(index>=0&&(NSUInteger)index<self.rows.count)[self.list selectRowIndexes:[NSIndexSet indexSetWithIndex:index] byExtendingSelection:NO];self.refreshing=NO;[self fields];}
- (BOOL)windowShouldClose:(NSWindow *)sender{(void)sender;stopUI();return YES;}
- (void)show{[self refresh];self.status.stringValue=value("status");[self.window makeKeyAndOrderFront:nil];[NSApp activateIgnoringOtherApps:YES];}
- (void)save{NSString *r=take(wb_save(self.name.stringValue.UTF8String,self.provider.stringValue.UTF8String,self.key.stringValue.UTF8String,self.model.stringValue.UTF8String,self.upstream.stringValue.UTF8String,[@[@"default",@"none",@"low",@"high",@"max"][self.effort.indexOfSelectedItem] UTF8String]));if([r hasPrefix:@"OK "])[self refresh];self.status.stringValue=r;}
- (void)change:(const char *)command{NSString *r=take(wb_command(command));if([r hasPrefix:@"OK "])[self refresh];self.status.stringValue=r;}
- (void)add{[self change:"add"];}- (void)duplicate{[self change:"duplicate"];}- (void)remove{[self change:"delete"];}
- (void)execute:(const char *)command{for(NSButton *b in self.buttons)b.enabled=NO;self.list.enabled=NO;self.status.stringValue=@"正在处理 / Working…";dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED,0),^{NSString *r=take(wb_command(command));dispatch_async(dispatch_get_main_queue(),^{for(NSButton *b in self.buttons)b.enabled=YES;self.list.enabled=YES;self.status.stringValue=r;});});}
- (void)test{[self execute:"test"];}- (void)install{[self execute:"install"];}- (void)restore{[self execute:"restore"];}
- (void)copyConfiguration{NSString *r=take(wb_command("copy"));if([r hasPrefix:@"OK "]){NSPasteboard *p=[NSPasteboard generalPasteboard];[p clearContents];[p setString:[r substringFromIndex:3] forType:NSPasteboardTypeString];self.status.stringValue=@"配置已复制 / Configuration copied";}else self.status.stringValue=r;}
- (void)quit{wb_free(wb_command("quit"));stopUI();}
@end
void wb_native_run(void){@autoreleasepool{[NSApplication sharedApplication];[NSApp setActivationPolicy:NSApplicationActivationPolicyAccessory];ui=[ProxyUI new];NSApp.delegate=ui;[NSApp run];ui=nil;}}
void wb_native_show(void){if(ui){dispatch_async(dispatch_get_main_queue(),^{[ui show];});}else{for(NSRunningApplication *app in [NSRunningApplication runningApplicationsWithBundleIdentifier:@"io.github.gloryhui.workbuddy-proxy"]){if(app.processIdentifier!=getpid())[app activateWithOptions:NSApplicationActivateIgnoringOtherApps];}}}
void wb_native_quit(void){dispatch_async(dispatch_get_main_queue(),^{[ui quit];});}
void wb_native_error(const char *message){@autoreleasepool{[NSApplication sharedApplication];NSAlert *alert=[NSAlert new];alert.messageText=@"WorkBuddy Proxy";alert.informativeText=[NSString stringWithUTF8String:message]?:@"Startup failed";[alert runModal];}}
