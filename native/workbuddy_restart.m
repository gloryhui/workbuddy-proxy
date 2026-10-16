#import <AppKit/AppKit.h>
#include <unistd.h>

// Called only by the settings worker after a user-selected successful Sync.
// All AppKit access runs on the main queue; waits never block the UI thread.
static int wb_restart_application(NSString *identifier, NSTimeInterval quitTimeout) {
    @autoreleasepool {
        __block NSRunningApplication *original = nil;
        __block NSURL *url = nil;
        __block int result = 0;
        dispatch_sync(dispatch_get_main_queue(), ^{
            NSArray<NSRunningApplication *> *apps =
                [NSRunningApplication runningApplicationsWithBundleIdentifier:identifier];
            if (apps.count > 1) { result = 2; return; }
            original = apps.firstObject;
            url = original.bundleURL ?: [[NSWorkspace sharedWorkspace]
                URLForApplicationWithBundleIdentifier:identifier];
            if (!url) { result = 1; return; }
            // Resolve the relaunch target before asking a running app to quit.
            if (![[[NSBundle bundleWithURL:url] bundleIdentifier] isEqualToString:identifier]) {
                result = 1; return;
            }
            if (original && !original.terminated && ![original terminate]) result = 3;
        });
        if (result) return result;

        NSTimeInterval deadline = NSProcessInfo.processInfo.systemUptime + quitTimeout;
        while (original) {
            __block BOOL exited = NO;
            dispatch_sync(dispatch_get_main_queue(), ^{ exited = original.terminated; });
            if (exited) break;
            if (NSProcessInfo.processInfo.systemUptime >= deadline) return 4;
            usleep(100000);
        }

        dispatch_semaphore_t done = dispatch_semaphore_create(0);
        __block BOOL launched = NO;
        dispatch_async(dispatch_get_main_queue(), ^{
            NSWorkspaceOpenConfiguration *options = [NSWorkspaceOpenConfiguration configuration];
            options.activates = YES;
            options.createsNewApplicationInstance = NO;
            [[NSWorkspace sharedWorkspace] openApplicationAtURL:url configuration:options
                completionHandler:^(NSRunningApplication *app, NSError *error) {
                    launched = app != nil && error == nil;
                    dispatch_semaphore_signal(done);
                }];
        });
        if (dispatch_semaphore_wait(done, dispatch_time(DISPATCH_TIME_NOW, 15 * NSEC_PER_SEC)))
            return 6;
        return launched ? 0 : 5;
    }
}

int wb_restart_workbuddy(void) {
    return wb_restart_application(@"com.tencent.workbuddy.mac", 20.);
}
