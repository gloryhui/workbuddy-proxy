/* QA-only identity routing. Never terminates or opens the user's WorkBuddy.
 * The product entry/algorithm and product binaries stay unchanged. Only this
 * test-process AppKit lookup identity is redirected to the owned Cocoa target.
 * NSBundle validation reports the requested identity for that ONE test bundle;
 * the OS-registered identity and actual target process remain unique test IDs.
 */
#import <AppKit/AppKit.h>
#import <objc/runtime.h>
#include <sys/stat.h>
#include <unistd.h>
#include <limits.h>
#include <stdlib.h>
#include <stdio.h>
static NSString *targetPath,*targetID,*proofPath,*modePath,*nonce;
static NSArray*(*oldApps)(id,SEL,NSString*);
static NSURL*(*oldURL)(id,SEL,NSString*);
static NSString*(*oldBundleID)(id,SEL);
static BOOL(*oldTerminate)(id,SEL);
static void(*oldOpen)(id,SEL,NSURL*,NSWorkspaceOpenConfiguration*,void(^)(NSRunningApplication*,NSError*));
static NSString *realID=@"com.tencent.workbuddy.mac";
static NSString *canonicalPath(NSString *p){char out[PATH_MAX];return p&&realpath(p.fileSystemRepresentation,out)?[NSString stringWithUTF8String:out]:nil;}
static BOOL ownURL(NSURL *url) {return url && [canonicalPath(url.path) isEqualToString:targetPath];}
static BOOL ownApp(NSRunningApplication *app) {return app && ownURL(app.bundleURL) && [app.bundleIdentifier isEqualToString:targetID];}
static BOOL routing(void) {
    NSData *d=[NSData dataWithContentsOfFile:modePath];NSDictionary *m=d?[NSJSONSerialization JSONObjectWithData:d options:0 error:nil]:nil;
    return [m[@"route_owned_target"] boolValue];
}
static void b05_audit(NSString *event, NSRunningApplication *app) {
    @synchronized(proofPath) {
        NSDictionary *row=@{@"event":event,@"process_pid":@(getpid()),@"target_pid":@(app?app.processIdentifier:0),@"target_path":targetPath,@"target_id":targetID,@"nonce":nonce};
        NSMutableData *d=[[NSJSONSerialization dataWithJSONObject:row options:0 error:nil] mutableCopy];[d appendBytes:"\n" length:1];
        if(![NSFileManager.defaultManager fileExistsAtPath:proofPath]) [NSFileManager.defaultManager createFileAtPath:proofPath contents:nil attributes:@{NSFilePosixPermissions:@0600}];
        NSFileHandle *f=[NSFileHandle fileHandleForWritingAtPath:proofPath];[f seekToEndOfFile];[f writeData:d];[f closeFile];
    }
}
static NSArray *apps(id obj,SEL sel,NSString *identifier) {
    if(![identifier isEqualToString:realID])return oldApps(obj,sel,identifier);
    if(!routing()){b05_audit(@"lookup_denied",nil);return @[];}
    NSArray *found=oldApps(obj,sel,targetID);
    for(NSRunningApplication *a in found)if(!ownApp(a)){b05_audit(@"unexpected_target_blocked",a);return @[];}
    b05_audit(@"lookup_redirected",found.firstObject);return found;
}
static NSURL *url(id obj,SEL sel,NSString *identifier) {
    if(![identifier isEqualToString:realID])return oldURL(obj,sel,identifier);
    if(!routing()){b05_audit(@"url_denied",nil);return nil;}
    b05_audit(@"url_redirected",nil);return [NSURL fileURLWithPath:targetPath];
}
static NSString *bundleID(id obj,SEL sel) {
    if(ownURL([(NSBundle*)obj bundleURL]))return realID;
    return oldBundleID(obj,sel);
}
static BOOL terminate(id obj,SEL sel) {
    if(!ownApp(obj)){b05_audit(@"termination_blocked",obj);return NO;}
    b05_audit(@"terminate_owned",obj);return oldTerminate(obj,sel);
}
static BOOL forceTerminate(id obj,SEL sel) {b05_audit(@"force_always_blocked",obj);return NO;}
static void openApp(id obj,SEL sel,NSURL *u,NSWorkspaceOpenConfiguration *cfg,void(^done)(NSRunningApplication*,NSError*)) {
    if(!ownURL(u)){b05_audit(@"open_blocked",nil);done(nil,[NSError errorWithDomain:@"B05OwnedTargetGuard" code:1 userInfo:nil]);return;}
    b05_audit(@"open_owned",nil);
    oldOpen(obj,sel,u,cfg,^(NSRunningApplication *app,NSError *error){
        if(app&&!ownApp(app)){b05_audit(@"unexpected_open_result",app);done(nil,[NSError errorWithDomain:@"B05OwnedTargetGuard" code:2 userInfo:nil]);return;}
        b05_audit(app?@"opened_owned":@"open_owned_failed",app);done(app,error);
    });
}
static IMP swap(Class cls,SEL selector,IMP replacement) {
    Method m=class_getInstanceMethod(cls,selector);if(!m){fprintf(stderr,"B05 missing method %s\n",sel_getName(selector));_exit(83);}return method_setImplementation(m,replacement);
}
__attribute__((constructor))static void initGuard(void) { @autoreleasepool {
    NSDictionary *env=NSProcessInfo.processInfo.environment;
    targetPath=env[@"B05_TARGET_APP"];targetID=env[@"B05_TARGET_ID"];proofPath=env[@"B05_GUARD_AUDIT"];modePath=env[@"B05_GUARD_MODE"];nonce=env[@"B05_GUARD_NONCE"];
    NSString *root=env[@"B05_OWN_ROOT"];
    if(!root||!targetPath||!targetID||!proofPath||!modePath||!nonce||![targetID hasPrefix:@"com.workbuddy.codexreview.b05target."]){fprintf(stderr,"B05 missing fields\n");_exit(79);}
    NSArray *parts=[targetPath pathComponents],*base=[root pathComponents];if(parts.count<=base.count||![[parts subarrayWithRange:NSMakeRange(0,base.count)] isEqualToArray:base]){fprintf(stderr,"B05 boundary mismatch\n");_exit(80);}
    if(![canonicalPath(targetPath) isEqualToString:targetPath]){fprintf(stderr,"B05 canonical mismatch\n");_exit(81);}
    struct stat st;if(lstat(targetPath.fileSystemRepresentation,&st)||st.st_uid!=getuid()||!S_ISDIR(st.st_mode)){fprintf(stderr,"B05 target ownership/type\n");_exit(82);}
    if(ownURL(NSBundle.mainBundle.bundleURL)){b05_audit(@"owned_target_no_interposition",nil);return;}
    oldApps=(void*)swap(object_getClass(NSRunningApplication.class),@selector(runningApplicationsWithBundleIdentifier:),(IMP)apps);
    oldURL=(void*)swap(NSWorkspace.class,@selector(URLForApplicationWithBundleIdentifier:),(IMP)url);
    oldBundleID=(void*)swap(NSBundle.class,@selector(bundleIdentifier),(IMP)bundleID);
    oldTerminate=(void*)swap(NSRunningApplication.class,@selector(terminate),(IMP)terminate);
    swap(NSRunningApplication.class,@selector(forceTerminate),(IMP)forceTerminate);
    oldOpen=(void*)swap(NSWorkspace.class,@selector(openApplicationAtURL:configuration:completionHandler:),(IMP)openApp);
    b05_audit(@"guard_installed",nil);
} }
