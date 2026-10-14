#import <AppKit/AppKit.h>
#import <CoreText/CoreText.h>
#include <unistd.h>
#include "icon_png.h"
#include "ui.h"
static NSString *take(char *p){NSString *s=p?[NSString stringWithUTF8String:p]:@"Internal error";wb_free(p);return s?:@"Internal error";}
static NSString *value(const char *n){return take(wb_value(n));}
static NSString *t(int n){return [NSString stringWithUTF8String:tr(n)];}
static NSColor *color(unsigned c){return [NSColor colorWithCalibratedRed:((c>>16)&255)/255.0 green:((c>>8)&255)/255.0 blue:(c&255)/255.0 alpha:1];}
static NSImage *appIcon(void){static NSImage *icon;static dispatch_once_t once;dispatch_once(&once,^{icon=[[NSImage alloc]initWithData:[NSData dataWithBytes:WB_ICON_PNG length:sizeof(WB_ICON_PNG)]];icon.accessibilityDescription=@"WorkBuddy Proxy";});return icon;}
@interface WBTextField:NSTextField
@end
@implementation WBTextField
- (void)mouseDown:(NSEvent *)event{[super mouseDown:event];if(event.clickCount==2)[[self currentEditor] selectAll:nil];}
@end
@interface WBFieldEditor:NSTextView
@end
@implementation WBFieldEditor
- (void)mouseDown:(NSEvent *)event{[super mouseDown:event];if(event.clickCount==2)[self selectAll:nil];}
@end
// Keep ordinary text editing/clipboard behavior; mask only the rendered glyphs.
@interface WBKeyCell:NSTextFieldCell
@property BOOL masked;
@end
@implementation WBKeyCell
- (void)drawInteriorWithFrame:(NSRect)frame inView:(NSView *)view {
    if(!self.masked){[super drawInteriorWithFrame:frame inView:view];return;}
    WBKeyCell *display=[self copy];display.masked=NO;display.stringValue=[@"" stringByPaddingToLength:self.stringValue.length withString:@"*" startingAtIndex:0];[display drawInteriorWithFrame:frame inView:view];
}
@end
@interface WBKeyField:WBTextField
@end
@implementation WBKeyField
+ (Class)cellClass{return WBKeyCell.class;}
@end
@interface WBKeyMask:NSObject<NSLayoutManagerDelegate>
@property BOOL masked;
@end
@implementation WBKeyMask
- (NSUInteger)layoutManager:(NSLayoutManager *)manager shouldGenerateGlyphs:(const CGGlyph *)glyphs properties:(const NSGlyphProperty *)properties characterIndexes:(const NSUInteger *)indexes font:(NSFont *)font forGlyphRange:(NSRange)range {
    (void)glyphs;if(!self.masked)return 0;UniChar star='*';CGGlyph glyph=0;CTFontGetGlyphsForCharacters((__bridge CTFontRef)font,&star,&glyph,1);NSMutableData *data=[NSMutableData dataWithLength:range.length*sizeof(CGGlyph)];CGGlyph *masked=data.mutableBytes;for(NSUInteger i=0;i<range.length;i++)masked[i]=glyph;[manager setGlyphs:masked properties:properties characterIndexes:indexes font:font forGlyphRange:range];return range.length;
}
@end
@protocol WBModelActions <NSObject>
- (void)removeModel;
- (void)addModel;
- (void)editModel;
- (void)copyModelIDs;
- (void)save;
@end
@interface WBModelTable:NSTableView
@property(nonatomic,weak) id<WBModelActions> actions;
@end
@implementation WBModelTable
- (void)keyDown:(NSEvent *)event {
    BOOL command=(event.modifierFlags&NSEventModifierFlagCommand)!=0;
    NSString *key=event.charactersIgnoringModifiers.lowercaseString;
    if(command&&[key isEqualToString:@"a"]){[self selectAll:nil];return;}
    if(command&&[key isEqualToString:@"c"]){[self.actions copyModelIDs];return;}
    if(command&&[key isEqualToString:@"s"]){[self.actions save];return;}
    if(command&&[key isEqualToString:@"n"]){[self.actions addModel];return;}
    if(event.keyCode==51||event.keyCode==117){[self.actions removeModel];return;}
    if(event.keyCode==36||event.keyCode==120){[self.actions editModel];return;}
    [super keyDown:event];
}
@end
@interface WBModelLabel:NSTextField
@end
@implementation WBModelLabel
- (NSView *)hitTest:(NSPoint)point{(void)point;return nil;}
@end
@interface WBProviderRow:NSTableRowView
@end
@implementation WBProviderRow
- (NSBackgroundStyle)interiorBackgroundStyle{return NSBackgroundStyleNormal;}
- (void)drawSelectionInRect:(NSRect)dirtyRect{(void)dirtyRect;}
@end
@interface WBProviderCell:NSView
@end
@implementation WBProviderCell
- (NSView *)hitTest:(NSPoint)point{(void)point;return nil;}
@end
@interface WBModelRow:NSTableRowView
@end
@implementation WBModelRow
- (NSBackgroundStyle)interiorBackgroundStyle{return NSBackgroundStyleNormal;}
- (void)drawSelectionInRect:(NSRect)dirtyRect {
    (void)dirtyRect;[color(0xcdeafb) setFill];[[NSBezierPath bezierPathWithRoundedRect:NSInsetRect(self.bounds,1,1) xRadius:5 yRadius:5] fill];
}
@end
@interface ProxyUI:NSObject<NSApplicationDelegate,NSWindowDelegate,NSTableViewDataSource,NSTableViewDelegate,NSTextFieldDelegate,WBModelActions>
@property NSWindow *window;
@property NSButton *updateButton;
@property NSButton *keyToggle;
@property WBKeyMask *keyMask;
@property NSTextView *keyEditor;
@property NSTextView *formEditor;
@property BOOL keyShown;
@property NSTextField *modelEditor;
@property BOOL updating;
@property NSStatusItem *item;
@property NSView *left,*center,*right,*emptyView;
@property NSImageView *emptyIcon;
@property NSButton *emptyAdd,*emptyRestore;
@property NSTableView *list,*models;
@property NSScrollView *providerScroll,*modelScroll,*logScroll;
@property NSArray<NSString *> *rows;
@property(nonatomic) BOOL refreshing,busy,polling,dirty;
@property NSTextField *name,*upstream,*status;
@property NSTextField *key;
@property NSTextView *logs;
@property NSPopUpButton *effort,*language;
@property NSMutableArray<NSControl *> *controls;
@property NSMutableArray<NSView *> *translated;
@property NSMutableArray<NSString *> *events;
@property NSString *systemText;
@property NSTimer *timer;
@end
static ProxyUI *ui;
static NSString *const WBShowNotification=@"io.github.gloryhui.workbuddy-proxy.show";
static void stopUI(void){[NSApp stop:nil];[NSApp postEvent:[NSEvent otherEventWithType:NSEventTypeApplicationDefined location:NSZeroPoint modifierFlags:0 timestamp:0 windowNumber:0 context:nil subtype:0 data1:0 data2:0] atStart:YES];}
@implementation ProxyUI
- (NSView *)card {NSView *v=[NSView new];v.wantsLayer=YES;v.layer.backgroundColor=color(0xffffff).CGColor;v.layer.cornerRadius=12;v.layer.borderWidth=1;v.layer.borderColor=color(0xe2e9f1).CGColor;[self.window.contentView addSubview:v];return v;}
- (NSTextField *)label:(int)tag in:(NSView *)parent {NSTextField *v=[NSTextField labelWithString:t(tag)];v.tag=tag;v.font=[NSFont systemFontOfSize:(tag==T_MODELS||tag==T_INFO||tag==T_PROVIDERS)?13:12 weight:(tag==T_MODELS||tag==T_INFO||tag==T_PROVIDERS)?NSFontWeightSemibold:NSFontWeightMedium];v.textColor=color(0x526379);[parent addSubview:v];[self.translated addObject:v];return v;}
- (NSTextField *)fieldIn:(NSView *)parent secure:(BOOL)masked {NSTextField *v=masked?[WBKeyField new]:[WBTextField new];if(masked){((WBKeyCell *)v.cell).masked=YES;v.contentType=@"";v.automaticTextCompletionEnabled=NO;self.keyMask=[WBKeyMask new];self.keyMask.masked=YES;}v.font=[NSFont systemFontOfSize:13];v.delegate=self;v.bezelStyle=NSTextFieldRoundedBezel;[parent addSubview:v];[self.controls addObject:v];return v;}
- (NSButton *)button:(int)tag action:(SEL)action in:(NSView *)parent {NSButton *v=[NSButton buttonWithTitle:t(tag) target:self action:action];v.tag=tag;v.bezelStyle=NSBezelStyleRounded;v.controlSize=NSControlSizeRegular;v.font=[NSFont systemFontOfSize:12 weight:NSFontWeightMedium];v.bordered=NO;v.wantsLayer=YES;v.layer.cornerRadius=6;v.layer.backgroundColor=color(0xedf3fa).CGColor;v.contentTintColor=color(0x355774);[parent addSubview:v];[self.controls addObject:v];[self.translated addObject:v];return v;}
- (void)frame:(NSView *)v x:(CGFloat)x top:(CGFloat)top width:(CGFloat)w height:(CGFloat)h in:(NSView *)parent {v.frame=NSMakeRect(x,parent.bounds.size.height-top-h,w,h);}
- (void)applicationDidFinishLaunching:(NSNotification *)note {
    (void)note;NSString *language=value("language");wb_use_language([language isEqualToString:@"zh"]||([language isEqualToString:@"auto"]&&[NSLocale.preferredLanguages.firstObject hasPrefix:@"zh"]));
    self.controls=[NSMutableArray array];self.translated=[NSMutableArray array];self.events=[NSMutableArray array];self.rows=@[];self.systemText=@"";
    self.window=[[NSWindow alloc]initWithContentRect:NSMakeRect(0,0,1136,730) styleMask:NSWindowStyleMaskTitled|NSWindowStyleMaskClosable|NSWindowStyleMaskMiniaturizable|NSWindowStyleMaskResizable backing:NSBackingStoreBuffered defer:NO];self.window.minSize=NSMakeSize(1080,680);self.window.title=@"WorkBuddy Proxy";self.window.delegate=self;self.window.releasedWhenClosed=NO;self.window.appearance=[NSAppearance appearanceNamed:NSAppearanceNameAqua];self.window.backgroundColor=color(0xf3f6fb);
    self.left=[self card];self.left.layer.backgroundColor=color(0xeef3f9).CGColor;self.left.layer.borderWidth=0;self.center=[self card];self.right=[self card];
    NSImageView *logo=[[NSImageView alloc]initWithFrame:NSMakeRect(24,666,40,40)];logo.image=appIcon();logo.imageScaling=NSImageScaleProportionallyUpOrDown;logo.tag=100;[self.window.contentView addSubview:logo];
    NSTextField *heading=[NSTextField labelWithString:@"WorkBuddy Proxy"];heading.font=[NSFont systemFontOfSize:21 weight:NSFontWeightSemibold];heading.textColor=color(0x142c48);heading.tag=101;[self.window.contentView addSubview:heading];[self label:T_SUBTITLE in:self.window.contentView];
    self.updateButton=[NSButton buttonWithTitle:@"" target:self action:@selector(updateClick)];self.updateButton.bezelStyle=NSBezelStyleRounded;self.updateButton.font=[NSFont systemFontOfSize:12];[self.window.contentView addSubview:self.updateButton];
    self.language=[NSPopUpButton new];[self.language addItemsWithTitles:@[@"简体中文",@"English"]];[self.language selectItemAtIndex:wb_ui_zh()?0:1];self.language.target=self;self.language.action=@selector(changeLanguage);[self.window.contentView addSubview:self.language];[self.controls addObject:self.language];
    [self label:T_PROVIDERS in:self.left];
    self.providerScroll=[NSScrollView new];self.providerScroll.hasVerticalScroller=YES;self.providerScroll.autohidesScrollers=YES;self.list=[NSTableView new];NSTableColumn *col=[[NSTableColumn alloc]initWithIdentifier:@"provider"];[self.list addTableColumn:col];self.list.headerView=nil;self.list.rowHeight=54;self.list.intercellSpacing=NSMakeSize(0,5);self.list.selectionHighlightStyle=NSTableViewSelectionHighlightStyleRegular;self.list.backgroundColor=color(0xeef3f9);self.providerScroll.drawsBackground=NO;self.list.dataSource=self;self.list.delegate=self;self.list.allowsEmptySelection=NO;self.list.style=NSTableViewStylePlain;self.providerScroll.documentView=self.list;[self.left addSubview:self.providerScroll];
    [self button:T_ADD action:@selector(add) in:self.left];[self button:T_DUPLICATE action:@selector(duplicate) in:self.left];[self button:T_DELETE action:@selector(remove) in:self.left];
    [self label:T_NAME in:self.center];self.name=[self fieldIn:self.center secure:NO];
    [self label:T_URL in:self.center];self.upstream=[self fieldIn:self.center secure:NO];self.upstream.placeholderString=@"https://example.com/v1";
    [self label:T_KEY in:self.center];self.key=[self fieldIn:self.center secure:YES];self.keyToggle=[NSButton buttonWithTitle:@"" target:self action:@selector(toggleKey)];self.keyToggle.bordered=NO;self.keyToggle.font=[NSFont systemFontOfSize:12];self.keyToggle.contentTintColor=color(0x0081a8);[self.center addSubview:self.keyToggle];[self.controls addObject:self.keyToggle];
    [self label:T_MODELS in:self.center];[self button:T_FETCH action:@selector(fetch) in:self.center];[self button:T_ADD_MODEL action:@selector(addModel) in:self.center];[self button:T_REMOVE_MODEL action:@selector(removeModel) in:self.center];
    self.modelScroll=[NSScrollView new];self.modelScroll.hasVerticalScroller=YES;self.modelScroll.autohidesScrollers=YES;self.modelScroll.borderType=NSNoBorder;self.modelScroll.wantsLayer=YES;self.modelScroll.layer.cornerRadius=6;self.modelScroll.layer.borderWidth=1;self.modelScroll.layer.borderColor=color(0xe2e9f1).CGColor;self.modelScroll.layer.masksToBounds=YES;self.models=[WBModelTable new];self.models.style=NSTableViewStylePlain;self.models.intercellSpacing=NSMakeSize(0,0);((WBModelTable *)self.models).actions=self;self.models.allowsMultipleSelection=YES;self.models.target=self;self.models.doubleAction=@selector(editClickedModel);for(NSString *id in @[@"name",@"model"]){NSTableColumn *c=[[NSTableColumn alloc]initWithIdentifier:id];[self.models addTableColumn:c];}self.models.rowHeight=30;self.models.usesAlternatingRowBackgroundColors=NO;self.models.backgroundColor=NSColor.whiteColor;self.models.dataSource=self;self.models.delegate=self;self.models.columnAutoresizingStyle=NSTableViewUniformColumnAutoresizingStyle;self.modelScroll.documentView=self.models;[self.center addSubview:self.modelScroll];
    [self label:T_MODEL_HINT in:self.center];[self label:T_EFFORT in:self.center];self.effort=[NSPopUpButton new];[self.center addSubview:self.effort];[self.controls addObject:self.effort];self.effort.target=self;self.effort.action=@selector(markDirty);
    NSButton *save=[self button:T_SAVE action:@selector(save) in:self.center];save.contentTintColor=NSColor.whiteColor;save.layer.backgroundColor=color(0x008fc4).CGColor;save.keyEquivalent=@"s";save.keyEquivalentModifierMask=NSEventModifierFlagCommand;
    [self button:T_TEST action:@selector(test) in:self.center];[self button:T_SYNC action:@selector(install) in:self.center];[self button:T_RESTORE action:@selector(restore) in:self.center];
    self.emptyView=[NSView new];[self.center addSubview:self.emptyView];self.emptyIcon=[NSImageView new];self.emptyIcon.image=appIcon();[self.emptyView addSubview:self.emptyIcon];NSTextField *emptyTitle=[self label:T_EMPTY_TITLE in:self.emptyView];emptyTitle.alignment=NSTextAlignmentCenter;emptyTitle.font=[NSFont systemFontOfSize:19 weight:NSFontWeightSemibold];NSTextField *emptyHint=[self label:T_EMPTY_HINT in:self.emptyView];emptyHint.alignment=NSTextAlignmentCenter;self.emptyAdd=[self button:T_ADD_PROVIDER action:@selector(add) in:self.emptyView];self.emptyAdd.layer.backgroundColor=color(0x008fc4).CGColor;self.emptyAdd.contentTintColor=NSColor.whiteColor;self.emptyRestore=[self button:T_RESTORE action:@selector(restore) in:self.emptyView];
    [self label:T_INFO in:self.right];self.logScroll=[NSScrollView new];self.logScroll.hasVerticalScroller=YES;self.logScroll.autohidesScrollers=YES;self.logs=[[NSTextView alloc]initWithFrame:NSMakeRect(0,0,260,550)];self.logs.autoresizingMask=NSViewWidthSizable;self.logs.horizontallyResizable=NO;self.logs.editable=NO;self.logs.richText=NO;self.logs.font=[NSFont monospacedSystemFontOfSize:11 weight:NSFontWeightRegular];self.logs.textColor=color(0x465d75);self.logs.backgroundColor=color(0xf8fafd);self.logs.textContainerInset=NSMakeSize(12,12);self.logs.verticallyResizable=YES;self.logs.textContainer.widthTracksTextView=YES;self.logScroll.documentView=self.logs;[self.right addSubview:self.logScroll];
    self.status=[NSTextField labelWithString:t(T_READY)];self.status.font=[NSFont systemFontOfSize:11];self.status.textColor=color(0x526379);self.status.lineBreakMode=NSLineBreakByTruncatingMiddle;[self.window.contentView addSubview:self.status];
    self.item=[[NSStatusBar systemStatusBar]statusItemWithLength:NSSquareStatusItemLength];NSImage *menuIcon=[appIcon() copy];menuIcon.size=NSMakeSize(18,18);self.item.button.image=menuIcon;
    [self translate];[self layout];[self.window center];[self refresh];[[NSDistributedNotificationCenter defaultCenter] addObserver:self selector:@selector(reopen:) name:WBShowNotification object:[NSString stringWithFormat:@"%d",getpid()] suspensionBehavior:NSNotificationSuspensionBehaviorDeliverImmediately];[self show];wb_frontend_ready();[self poll];[self checkUpdate:NO];self.timer=[NSTimer scheduledTimerWithTimeInterval:2 target:self selector:@selector(poll) userInfo:nil repeats:YES];
}
- (NSView *)tag:(int)tag in:(NSView *)parent {for(NSView *v in parent.subviews)if([v isKindOfClass:NSControl.class]&&v.tag==tag)return v;return nil;}
- (void)layout {
    CGFloat w=self.window.contentView.bounds.size.width,h=self.window.contentView.bounds.size.height,left=202,right=340,gap=14,margin=20,center=w-2*margin-left-right-2*gap,ch=h-112,extra=0;
    self.left.frame=NSMakeRect(margin,40,left,ch);self.center.frame=NSMakeRect(margin+left+gap,40,center,ch);self.right.frame=NSMakeRect(w-margin-right,40,right,ch);
    [self frame:[self tag:100 in:self.window.contentView] x:24 top:16 width:36 height:36 in:self.window.contentView];[self frame:[self tag:101 in:self.window.contentView] x:72 top:14 width:390 height:26 in:self.window.contentView];[self frame:[self tag:T_SUBTITLE in:self.window.contentView] x:73 top:42 width:500 height:18 in:self.window.contentView];[self frame:self.updateButton x:w-304 top:24 width:140 height:28 in:self.window.contentView];[self frame:self.language x:w-150 top:24 width:126 height:28 in:self.window.contentView];
    [self frame:[self tag:T_PROVIDERS in:self.left] x:10 top:16 width:left-20 height:20 in:self.left];[self frame:self.providerScroll x:2 top:48 width:left-4 height:ch-108 in:self.left];self.list.tableColumns[0].width=self.providerScroll.contentSize.width;
    int tags[]={T_ADD,T_DUPLICATE,T_DELETE};for(int i=0;i<3;i++)[self frame:[self tag:tags[i] in:self.left] x:10+i*62 top:ch-46 width:58 height:30 in:self.left];
    CGFloat nw=(center-48)*.38;
    [self frame:[self tag:T_NAME in:self.center] x:18 top:18 width:nw height:20 in:self.center];[self frame:self.name x:18 top:44 width:nw height:32 in:self.center];
    [self frame:[self tag:T_KEY in:self.center] x:30+nw top:18 width:center-48-nw height:20 in:self.center];[self frame:self.key x:30+nw top:44 width:center-92-nw height:32 in:self.center];[self frame:self.keyToggle x:center-56 top:44 width:38 height:32 in:self.center];
    [self frame:[self tag:T_URL in:self.center] x:18 top:94 width:center-36 height:20 in:self.center];[self frame:self.upstream x:18 top:120 width:center-36 height:32 in:self.center];
    [self frame:[self tag:T_MODELS in:self.center] x:18 top:186 width:120 height:22 in:self.center];[self frame:[self tag:T_FETCH in:self.center] x:center-218 top:180 width:114 height:30 in:self.center];[self frame:[self tag:T_ADD_MODEL in:self.center] x:center-94 top:180 width:32 height:30 in:self.center];[self frame:[self tag:T_REMOVE_MODEL in:self.center] x:center-50 top:180 width:32 height:30 in:self.center];
    [self frame:self.modelScroll x:18 top:224 width:center-36 height:ch-362-extra in:self.center];CGFloat modelWidth=self.modelScroll.contentSize.width;self.models.tableColumns[0].width=modelWidth*.36;self.models.tableColumns[1].width=modelWidth*.64;
    [self frame:[self tag:T_MODEL_HINT in:self.center] x:18 top:ch-128-extra width:center-36 height:18 in:self.center];[self frame:[self tag:T_EFFORT in:self.center] x:18 top:ch-94-extra width:110 height:20 in:self.center];[self frame:self.effort x:128 top:ch-100-extra width:center-146 height:30 in:self.center];
    int actions[]={T_SAVE,T_TEST,T_SYNC,T_RESTORE};CGFloat widths[]={72,108,116,84};CGFloat x=18;for(int i=0;i<4;i++){[self frame:[self tag:actions[i] in:self.center] x:x top:ch-52-((!extra||i>=3)?0:extra) width:widths[i] height:32 in:self.center];x+=widths[i]+6;}
    self.emptyView.frame=self.center.bounds;CGFloat emptyTop=ch*.44;self.emptyIcon.frame=NSMakeRect((center-44)/2,ch-emptyTop+46,44,44);[self frame:[self tag:T_EMPTY_TITLE in:self.emptyView] x:24 top:emptyTop width:center-48 height:28 in:self.emptyView];[self frame:[self tag:T_EMPTY_HINT in:self.emptyView] x:18 top:emptyTop+38 width:center-36 height:22 in:self.emptyView];[self frame:self.emptyAdd x:(center-140)/2 top:emptyTop+86 width:140 height:34 in:self.emptyView];[self frame:self.emptyRestore x:(center-140)/2 top:emptyTop+132 width:140 height:30 in:self.emptyView];
    [self frame:[self tag:T_INFO in:self.right] x:16 top:16 width:right-32 height:20 in:self.right];[self frame:self.logScroll x:10 top:48 width:right-20 height:ch-60 in:self.right];self.logs.minSize=NSMakeSize(0,self.logScroll.contentSize.height);self.logs.maxSize=NSMakeSize(self.logScroll.contentSize.width,CGFLOAT_MAX);self.logs.frame=NSMakeRect(0,0,self.logScroll.contentSize.width,MAX(self.logs.bounds.size.height,self.logScroll.contentSize.height));self.status.frame=NSMakeRect(24,12,w-48,18);
}
- (void)windowDidResize:(NSNotification *)note{(void)note;[self layout];}
- (NSText *)windowWillReturnFieldEditor:(NSWindow *)sender toObject:(id)client {
    (void)sender;if(client==self.key){if(!self.keyEditor){self.keyEditor=[[WBFieldEditor alloc]initWithFrame:NSZeroRect];self.keyEditor.fieldEditor=YES;self.keyEditor.richText=NO;self.keyEditor.automaticTextCompletionEnabled=NO;self.keyEditor.layoutManager.delegate=self.keyMask;}return self.keyEditor;}if(client==self.name||client==self.upstream){if(!self.formEditor){self.formEditor=[[WBFieldEditor alloc]initWithFrame:NSZeroRect];self.formEditor.fieldEditor=YES;self.formEditor.richText=NO;}return self.formEditor;}return nil;
}
- (void)renderKey {
    ((WBKeyCell *)self.key.cell).masked=!self.keyShown;self.keyMask.masked=!self.keyShown;[self.key setNeedsDisplay:YES];if(self.keyEditor){[self.keyEditor.layoutManager invalidateGlyphsForCharacterRange:NSMakeRange(0,self.keyEditor.string.length) changeInLength:0 actualCharacterRange:NULL];self.keyEditor.needsDisplay=YES;}NSString *label=wb_ui_zh()?(self.keyShown?@"隐藏 API Key":@"显示 API Key"):(self.keyShown?@"Hide API Key":@"Show API Key");self.keyToggle.title=@"";self.keyToggle.image=[NSImage imageWithSystemSymbolName:self.keyShown?@"eye.slash":@"eye" accessibilityDescription:label];self.keyToggle.imagePosition=NSImageOnly;self.keyToggle.toolTip=label;self.keyToggle.accessibilityLabel=label;
}
- (void)toggleKey {self.keyShown=!self.keyShown;[self renderKey];}
- (void)installMainMenu {
    BOOL zh=wb_ui_zh();NSMenu *main=[NSMenu new],*app=[NSMenu new],*edit=[[NSMenu alloc]initWithTitle:zh?@"编辑":@"Edit"];
    NSMenuItem *appItem=[[NSMenuItem alloc]initWithTitle:@"WorkBuddy Proxy" action:nil keyEquivalent:@""];appItem.submenu=app;[main addItem:appItem];
    NSMenuItem *about=[[NSMenuItem alloc]initWithTitle:zh?@"关于 WorkBuddy Proxy":@"About WorkBuddy Proxy" action:@selector(orderFrontStandardAboutPanel:) keyEquivalent:@""];about.target=NSApp;[app addItem:about];[app addItem:NSMenuItem.separatorItem];
    NSMenuItem *hide=[[NSMenuItem alloc]initWithTitle:zh?@"隐藏 WorkBuddy Proxy":@"Hide WorkBuddy Proxy" action:@selector(hide:) keyEquivalent:@"h"];hide.target=NSApp;[app addItem:hide];[app addItem:NSMenuItem.separatorItem];
    NSMenuItem *quit=[[NSMenuItem alloc]initWithTitle:t(T_QUIT) action:@selector(quit) keyEquivalent:@"q"];quit.target=self;[app addItem:quit];
    NSMenuItem *editItem=[[NSMenuItem alloc]initWithTitle:edit.title action:nil keyEquivalent:@""];editItem.submenu=edit;[main addItem:editItem];
    NSArray *titles=zh?@[@"撤销",@"重做",@"剪切",@"复制",@"粘贴",@"全选"]:@[@"Undo",@"Redo",@"Cut",@"Copy",@"Paste",@"Select All"];
    SEL actions[]={@selector(undo:),@selector(redo:),@selector(cut:),@selector(copy:),@selector(paste:),@selector(selectAll:)};NSArray *keys=@[@"z",@"z",@"x",@"c",@"v",@"a"];
    for(int i=0;i<6;i++){if(i==2||i==5)[edit addItem:NSMenuItem.separatorItem];NSMenuItem *item=[[NSMenuItem alloc]initWithTitle:titles[i] action:actions[i] keyEquivalent:keys[i]];if(i==1)item.keyEquivalentModifierMask=NSEventModifierFlagCommand|NSEventModifierFlagShift;[edit addItem:item];}
    NSApp.mainMenu=main;
}
- (void)translate {[self installMainMenu];[self renderKey];for(NSView *v in self.translated){if([v isKindOfClass:NSButton.class]){NSButton *b=(NSButton *)v;b.title=v.tag==T_ADD_MODEL?@"+":v.tag==T_REMOVE_MODEL?@"−":t((int)v.tag);b.toolTip=t(v.tag==T_TEST?T_TEST_HINT:(int)v.tag);b.accessibilityLabel=t((int)v.tag);}else [(NSTextField *)v setStringValue:t((int)v.tag)];}NSInteger effort=MAX(0,self.effort.indexOfSelectedItem);[self.effort removeAllItems];for(int i=0;i<6;i++)[self.effort addItemWithTitle:t(T_DEFAULT+i)];[self.effort selectItemAtIndex:effort];self.key.placeholderString=t(T_KEY_HINT);self.key.toolTip=t(T_KEY_HINT);[self tag:T_FETCH in:self.center].toolTip=t(T_FETCH_HINT);[(NSTextField *)[self tag:T_MODEL_HINT in:self.center] setStringValue:wb_ui_zh()?@"⇧ 连续多选 · ⌘ 多选 · ⌫ 删除 · 双击编辑":@"Shift/⌘ select · Delete remove · Double-click edit"];self.models.tableColumns[0].title=t(T_DISPLAY);self.models.tableColumns[1].title=t(T_ID);self.models.tableColumns[0].headerCell.alignment=NSTextAlignmentLeft;self.models.tableColumns[1].headerCell.alignment=NSTextAlignmentLeft;self.models.tableColumns[0].headerCell.stringValue=t(T_DISPLAY);self.models.tableColumns[1].headerCell.stringValue=t(T_ID);self.models.headerView.needsDisplay=YES;[self.models reloadData];self.language.accessibilityLabel=t(T_LANGUAGE);NSMenu *menu=[NSMenu new];for(int n=0;n<2;n++){NSMenuItem *m=[[NSMenuItem alloc]initWithTitle:t(n?T_QUIT:T_SETTINGS) action:n?@selector(quit):@selector(show) keyEquivalent:n?@"q":@""];m.target=self;[menu addItem:m];}self.item.menu=menu;[(NSTextField *)[self tag:T_SUBTITLE in:self.window.contentView] setStringValue:[NSString stringWithFormat:@"%@ · v%@",t(T_SUBTITLE),value("version")]];self.updateButton.title=value("update_label");[self updateProviderState];}
- (void)checkUpdate:(BOOL)force {
    if(self.updating)return;self.updating=YES;self.updateButton.enabled=NO;self.updateButton.title=t(T_WORKING);
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_UTILITY,0),^{NSString *result=take(wb_check_update(force));dispatch_async(dispatch_get_main_queue(),^{self.updating=NO;self.updateButton.enabled=YES;self.updateButton.title=value("update_label");if(force)[self report:result];else {if(self.events.count==20)[self.events removeObjectAtIndex:0];[self.events addObject:[result hasPrefix:@"OK "]?[result substringFromIndex:3]:result];[self renderLog];}});});
}
- (void)updateClick {if([value("update_available") isEqualToString:@"1"]){NSURL *url=[NSURL URLWithString:value("update_url")];if(url)[NSWorkspace.sharedWorkspace openURL:url];}else [self checkUpdate:YES];}
- (void)changeLanguage {[self.window makeFirstResponder:nil];wb_use_language(self.language.indexOfSelectedItem==0);NSString *r=take(wb_language(wb_ui_zh()?"zh":"en"));[self.events removeAllObjects];self.systemText=@"";[self translate];[self reloadProviders];[self report:r];[self poll];}
- (NSInteger)numberOfRowsInTableView:(NSTableView *)table {return table==self.list?self.rows.count:wb_models_count();}
- (NSView *)tableView:(NSTableView *)table viewForTableColumn:(NSTableColumn *)column row:(NSInteger)row {
    if(table==self.list){
        NSString *text=self.rows[row];NSRange separator=[text rangeOfString:@" · " options:NSBackwardsSearch];NSString *name=separator.location==NSNotFound?text:[text substringToIndex:separator.location];NSString *count=separator.location==NSNotFound?@"":[text substringFromIndex:NSMaxRange(separator)];
        CGFloat width=column.width;BOOL selected=row==self.list.selectedRow;
        NSView *card=[[WBProviderCell alloc]initWithFrame:NSMakeRect(0,0,width,54)];card.wantsLayer=YES;card.layer.cornerRadius=8;card.layer.backgroundColor=(selected?color(0xc7e8fa):NSColor.clearColor).CGColor;card.layer.borderWidth=selected?1:0;card.layer.borderColor=color(0x58acd3).CGColor;
        if(selected){NSView *accent=[[NSView alloc]initWithFrame:NSMakeRect(1,8,3,38)];accent.wantsLayer=YES;accent.layer.cornerRadius=1.5;accent.layer.backgroundColor=color(0x008fc4).CGColor;[card addSubview:accent];}
        NSTextField *a=[NSTextField labelWithString:name];a.font=[NSFont systemFontOfSize:13 weight:NSFontWeightSemibold];a.textColor=selected?color(0x124e72):color(0x25465f);a.lineBreakMode=NSLineBreakByTruncatingTail;a.alignment=NSTextAlignmentLeft;a.frame=NSMakeRect(8,28,width-16,18);a.autoresizingMask=NSViewWidthSizable;a.toolTip=name;[card addSubview:a];
        NSTextField *b=[NSTextField labelWithString:count];b.font=[NSFont systemFontOfSize:11];b.textColor=selected?color(0x317b9f):color(0x6a8097);b.alignment=NSTextAlignmentLeft;b.frame=NSMakeRect(8,8,width-16,16);b.autoresizingMask=NSViewWidthSizable;[card addSubview:b];return card;
    }
    int col=[column.identifier isEqualToString:@"name"]?0:1;
    NSTableCellView *cell=[[NSTableCellView alloc]initWithFrame:NSMakeRect(0,0,column.width,table.rowHeight)];
    NSTextField *v=[WBModelLabel new];v.stringValue=take(wb_model((int)row,col));v.bezeled=NO;v.drawsBackground=NO;v.editable=NO;v.selectable=NO;v.font=[NSFont systemFontOfSize:13];v.lineBreakMode=NSLineBreakByTruncatingTail;v.alignment=NSTextAlignmentLeft;v.delegate=self;v.tag=1000+row*2+col;v.textColor=color(0x243b55);v.toolTip=v.stringValue;v.accessibilityLabel=[NSString stringWithFormat:@"%@ %ld",column.title,(long)row+1];
    CGFloat height=v.intrinsicContentSize.height;v.frame=NSMakeRect(8,floor((table.rowHeight-height)/2),MAX(0,column.width-16),height);v.autoresizingMask=NSViewWidthSizable|NSViewMinYMargin|NSViewMaxYMargin;cell.textField=v;[cell addSubview:v];return cell;
}
- (NSTableRowView *)tableView:(NSTableView *)table rowViewForRow:(NSInteger)row {(void)row;return table==self.models?[WBModelRow new]:[WBProviderRow new];}
- (void)editRow:(NSInteger)row column:(NSInteger)col {
    if(self.busy)return;[self.window makeFirstResponder:nil];if(row<0||col<0||row>=wb_models_count())return;
    NSTableCellView *cell=[self.models viewAtColumn:col row:row makeIfNecessary:YES];
    NSTextField *editor=[[NSTextField alloc]initWithFrame:NSInsetRect(cell.bounds,4,2)];editor.stringValue=take(wb_model((int)row,(int)col));editor.font=[NSFont systemFontOfSize:13];editor.alignment=NSTextAlignmentLeft;editor.editable=YES;editor.selectable=YES;editor.bezelStyle=NSTextFieldRoundedBezel;editor.delegate=self;editor.tag=1000+row*2+col;editor.accessibilityLabel=cell.textField.accessibilityLabel;self.modelEditor=editor;cell.textField.hidden=YES;[cell addSubview:editor];[self.window makeFirstResponder:editor];[[editor currentEditor] selectAll:self];
}
- (void)editModel {[self editRow:self.models.selectedRow column:0];}
- (void)editClickedModel {[self editRow:self.models.clickedRow column:self.models.clickedColumn];}
- (void)copyModelIDs {NSMutableArray *ids=[NSMutableArray array];[self.models.selectedRowIndexes enumerateIndexesUsingBlock:^(NSUInteger row,BOOL *stop){(void)stop;[ids addObject:take(wb_model((int)row,1))];}];if(!ids.count)return;NSPasteboard *p=NSPasteboard.generalPasteboard;[p clearContents];[p setString:[ids componentsJoinedByString:@"\n"] forType:NSPasteboardTypeString];}
- (BOOL)control:(NSControl *)control textView:(NSTextView *)editor doCommandBySelector:(SEL)command {if(control.tag>=1000&&command==@selector(cancelOperation:)){NSString *original=take(wb_model((int)(control.tag-1000)/2,(int)(control.tag-1000)%2));((NSTextField *)control).stringValue=original;editor.string=original;[self.window makeFirstResponder:self.models];return YES;}return NO;}
- (void)controlTextDidChange:(NSNotification *)note{if(!self.refreshing)self.dirty=YES;(void)note;}
- (void)controlTextDidEndEditing:(NSNotification *)note {
    NSTextField *v=note.object;if(v!=self.modelEditor)return;int row=(int)(v.tag-1000)/2,col=(int)(v.tag-1000)%2;wb_model_set(row,col,v.stringValue.UTF8String);NSTableCellView *cell=(NSTableCellView *)v.superview;cell.textField.stringValue=take(wb_model(row,col));cell.textField.toolTip=cell.textField.stringValue;cell.textField.hidden=NO;self.modelEditor=nil;[v removeFromSuperview];if(col==1){NSTableCellView *nameCell=[self.models viewAtColumn:0 row:row makeIfNecessary:NO];nameCell.textField.stringValue=take(wb_model(row,0));nameCell.textField.toolTip=nameCell.textField.stringValue;}
}
- (void)markDirty {self.dirty=YES;}
- (void)tableViewSelectionDidChange:(NSNotification *)note{NSInteger selectedRow=self.list.selectedRow;if(note.object!=self.list||self.refreshing||selectedRow<0)return;[self.window makeFirstResponder:nil];if(self.dirty){NSAlert *a=[NSAlert new];a.messageText=t(T_DISCARD);[a addButtonWithTitle:wb_ui_zh()?@"放弃修改":@"Discard"];[a addButtonWithTitle:wb_ui_zh()?@"取消":@"Cancel"];if([a runModal]!=NSAlertFirstButtonReturn){[self reloadProviders];return;}}wb_select((int)selectedRow);[self fields];[self reloadProviders];[self.window makeFirstResponder:self.list];}
- (void)fields{[self.window makeFirstResponder:nil];self.refreshing=YES;self.name.stringValue=value("name");self.upstream.stringValue=value("upstream");self.key.stringValue=value("api_key");self.keyShown=NO;[self renderKey];wb_models_load();[self.models deselectAll:nil];[self.models reloadData];NSString *e=value("reasoning_effort");for(int i=0;i<6;i++)if([e isEqualToString:[NSString stringWithUTF8String:efforts[i]]])[self.effort selectItemAtIndex:i];self.dirty=NO;self.refreshing=NO;[self updateProviderState];}
- (void)reloadProviders{self.refreshing=YES;NSString *rows=value("connections");self.rows=rows.length?[rows componentsSeparatedByString:@"\n"]:@[];[self.list reloadData];NSInteger i=[value("selected") integerValue];if(i>=0&&(NSUInteger)i<self.rows.count)[self.list selectRowIndexes:[NSIndexSet indexSetWithIndex:i] byExtendingSelection:NO];else [self.list deselectAll:nil];if(self.rows.count)[self.list reloadDataForRowIndexes:[NSIndexSet indexSetWithIndex:i] columnIndexes:[NSIndexSet indexSetWithIndex:0]];self.refreshing=NO;}
- (void)refresh{[self reloadProviders];[self fields];}
- (void)renderLog{NSPoint origin=self.logScroll.contentView.bounds.origin;BOOL bottom=NSMaxY(self.logScroll.documentVisibleRect)>=self.logs.bounds.size.height-35;self.logs.string=[self.systemText stringByAppendingFormat:@"\n\n%@\n%@",t(T_EVENTS),[self.events componentsJoinedByString:@"\n"]];if(bottom)[self.logs scrollRangeToVisible:NSMakeRange(self.logs.string.length,0)];else [self.logScroll.contentView scrollToPoint:origin];}
- (void)report:(NSString *)s {BOOL error=[s hasPrefix:@"ERROR "];if([s hasPrefix:@"OK "])s=[s substringFromIndex:3];self.status.textColor=error?color(0xc43a54):color(0x0081a8);self.status.stringValue=s;self.status.toolTip=s;if(self.events.count==20)[self.events removeObjectAtIndex:0];[self.events addObject:s];[self renderLog];}
- (void)poll{if(self.polling||self.busy||(!self.window.visible||self.window.miniaturized))return;self.polling=YES;int language=wb_ui_zh();dispatch_async(dispatch_get_global_queue(QOS_CLASS_UTILITY,0),^{NSString *s=take(wb_snapshot());dispatch_async(dispatch_get_main_queue(),^{self.polling=NO;if(language==wb_ui_zh()){self.systemText=s;[self renderLog];}else [self poll];});});}
- (BOOL)windowShouldClose:(NSWindow *)sender{(void)sender;[self.timer invalidate];wb_frontend_closing();stopUI();return YES;}
- (void)show{if(self.window.miniaturized)[self.window deminiaturize:nil];[self.window makeKeyAndOrderFront:nil];[NSApp activateIgnoringOtherApps:YES];}
- (void)reopen:(NSNotification *)note{(void)note;[self show];}
- (BOOL)applicationShouldHandleReopen:(NSApplication *)application hasVisibleWindows:(BOOL)visible{(void)application;(void)visible;[self show];return NO;}
- (void)save{if(self.busy||![self hasProvider])return;[self.window makeFirstResponder:nil];NSString *r=take(wb_save_table(self.name.stringValue.UTF8String,self.key.stringValue.UTF8String,self.upstream.stringValue.UTF8String,efforts[MAX(0,self.effort.indexOfSelectedItem)]));if([r hasPrefix:@"OK "])[self refresh];[self report:r];[self poll];}
- (void)change:(const char *)command{NSString *r=take(wb_command(command));if([r hasPrefix:@"OK "])[self refresh];[self report:r];[self poll];}
- (void)add{[self change:"add"];}- (void)duplicate{if(!self.busy&&[self hasProvider])[self change:"duplicate"];}
- (void)remove{if(self.busy||![self hasProvider])return;NSAlert *a=[NSAlert new];a.messageText=t(T_DELETE_QUESTION);[a addButtonWithTitle:t(T_DELETE)];[a addButtonWithTitle:wb_ui_zh()?@"取消":@"Cancel"];if([a runModal]==NSAlertFirstButtonReturn)[self change:"delete"];}
- (void)addModel{if(self.busy||![self hasProvider])return;[self.window makeFirstResponder:nil];int row=wb_model_add();if(row<0){[self report:t(T_LIMIT)];return;}self.dirty=YES;[self.models reloadData];[self.models selectRowIndexes:[NSIndexSet indexSetWithIndex:row] byExtendingSelection:NO];[self.models scrollRowToVisible:row];[self editRow:row column:1];}
- (void)removeModel{if(self.busy)return;[self.window makeFirstResponder:nil];NSIndexSet *rows=[self.models.selectedRowIndexes copy];if(!rows.count)return;[rows enumerateIndexesWithOptions:NSEnumerationReverse usingBlock:^(NSUInteger row,BOOL *stop){(void)stop;wb_model_remove((int)row);}];[self.models deselectAll:nil];[self.models reloadData];NSInteger next=MIN((NSInteger)rows.firstIndex,wb_models_count()-1);if(next>=0)[self.models selectRowIndexes:[NSIndexSet indexSetWithIndex:next] byExtendingSelection:NO];self.dirty=YES;[self.window makeFirstResponder:self.models];}
- (BOOL)hasProvider{return [value("has_provider") isEqualToString:@"1"];}
- (void)updateProviderState {
    BOOL has=[self hasProvider],installed=[value("installed_count") integerValue]>0;
    for(NSView *v in self.center.subviews)v.hidden=v==self.emptyView?has:!has;
    self.emptyRestore.hidden=!installed;
    for(NSControl *c in self.controls){BOOL needsProvider=([c isDescendantOf:self.center]&&![c isDescendantOf:self.emptyView])||c.tag==T_DUPLICATE||c.tag==T_DELETE;c.enabled=!self.busy&&(!needsProvider||has);}
    self.emptyAdd.enabled=!self.busy;self.emptyRestore.enabled=!self.busy&&installed;self.list.enabled=!self.busy;self.models.enabled=!self.busy&&has;
}
- (void)setControlsBusy:(BOOL)busy{self.busy=busy;[self updateProviderState];}
- (void)execute:(const char *)command{if(self.busy||(![self hasProvider]&&strcmp(command,"restore")!=0))return;[self.window makeFirstResponder:nil];[self setControlsBusy:YES];self.status.stringValue=t(T_WORKING);NSString *url=self.upstream.stringValue,*key=self.key.stringValue;BOOL fetch=strcmp(command,"fetch")==0;dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED,0),^{NSString *r=take(fetch?wb_fetch(url.UTF8String,key.UTF8String):wb_command(command));dispatch_async(dispatch_get_main_queue(),^{[self setControlsBusy:NO];if(fetch&&[r hasPrefix:@"OK "]){[self.models reloadData];self.dirty=YES;}[self report:r];[self poll];});});}
- (void)fetch{[self execute:"fetch"];}- (void)test{[self execute:"test"];}- (void)install{[self execute:"install"];}- (void)restore{[self execute:"restore"];}
- (NSApplicationTerminateReply)applicationShouldTerminate:(NSApplication *)sender{(void)sender;[self quit];return NSTerminateCancel;}
- (void)quit{wb_free(wb_command("quit"));[self.timer invalidate];wb_frontend_closing();stopUI();}
@end
void wb_native_run(void){@autoreleasepool{[NSApplication sharedApplication];NSApp.applicationIconImage=appIcon();[NSApp setActivationPolicy:NSApplicationActivationPolicyRegular];ui=[ProxyUI new];NSApp.delegate=ui;[NSApp run];[[NSDistributedNotificationCenter defaultCenter] removeObserver:ui];ui=nil;}}
int wb_native_show_process(unsigned int pid){@autoreleasepool{NSRunningApplication *app=[NSRunningApplication runningApplicationWithProcessIdentifier:(pid_t)pid];if(!app||app.terminated)return 0;[[NSDistributedNotificationCenter defaultCenter] postNotificationName:WBShowNotification object:[NSString stringWithFormat:@"%u",pid] userInfo:nil deliverImmediately:YES];[app activateWithOptions:NSApplicationActivateIgnoringOtherApps];return 1;}}
void wb_native_show(void){if(ui){dispatch_async(dispatch_get_main_queue(),^{[ui show];});}}
void wb_native_quit(void){dispatch_async(dispatch_get_main_queue(),^{[ui quit];});}
void wb_native_error(const char *message){@autoreleasepool{wb_use_language([NSLocale.preferredLanguages.firstObject hasPrefix:@"zh"]);[NSApplication sharedApplication];NSAlert *a=[NSAlert new];a.icon=appIcon();a.messageText=@"WorkBuddy Proxy";a.informativeText=take(wb_message(message));[a runModal];}}
