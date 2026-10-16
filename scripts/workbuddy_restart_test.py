#!/usr/bin/env python3
"""macOS native lifecycle tests using a disposable Cocoa app; never touches WorkBuddy."""
import json
from pathlib import Path
import plistlib
import subprocess
import sys
import tempfile
import time
import uuid


def main():
    if sys.platform != 'darwin':
        raise SystemExit('This native fixture requires macOS and Xcode command line tools')
    root = Path(__file__).resolve().parents[1]
    with tempfile.TemporaryDirectory(prefix='wbp-restart-') as folder:
        directory = Path(folder)
        identifier = 'io.github.gloryhui.workbuddy-proxy.fixture.' + uuid.uuid4().hex
        app = directory / 'Restart Fixture.app'
        contents = app / 'Contents'
        executable = contents / 'MacOS' / 'fixture'
        resources = contents / 'Resources'
        executable.parent.mkdir(parents=True)
        resources.mkdir()
        (contents / 'Info.plist').write_bytes(plistlib.dumps({
            'CFBundleIdentifier': identifier, 'CFBundleExecutable': 'fixture',
            'CFBundleName': 'Proxy Restart Fixture', 'CFBundlePackageType': 'APPL',
            'LSUIElement': True,
        }))
        fixture = directory / 'fixture.m'
        fixture.write_text(r'''
#import <AppKit/AppKit.h>
#include <unistd.h>
@interface Fixture : NSObject<NSApplicationDelegate>
@end
@implementation Fixture
- (void)applicationDidFinishLaunching:(NSNotification *)note {
    NSString *file = [[NSBundle.mainBundle resourcePath] stringByAppendingPathComponent:@"pid"];
    [[NSString stringWithFormat:@"%d", getpid()] writeToFile:file atomically:YES
        encoding:NSUTF8StringEncoding error:nil];
}
- (NSApplicationTerminateReply)applicationShouldTerminate:(NSApplication *)sender {
    NSString *file = [[NSBundle.mainBundle resourcePath] stringByAppendingPathComponent:@"refuse"];
    return [NSFileManager.defaultManager fileExistsAtPath:file] ? NSTerminateCancel : NSTerminateNow;
}
@end
int main(void) { @autoreleasepool {
    NSApplication *app = NSApplication.sharedApplication;
    Fixture *delegate = [Fixture new]; app.delegate = delegate; [app run];
} }
''')
        driver_source = directory / 'driver.m'
        driver_source.write_text(r'''
#import "workbuddy_restart.m"
#include <stdio.h>
int main(int argc, const char **argv) { @autoreleasepool {
    if (argc != 3) return 99;
    [NSApplication sharedApplication];
    NSString *identifier = [NSString stringWithUTF8String:argv[1]];
    NSString *mode = [NSString stringWithUTF8String:argv[2]];
    if ([mode isEqualToString:@"count"] || [mode isEqualToString:@"quit"]) {
        NSArray *apps = [NSRunningApplication runningApplicationsWithBundleIdentifier:identifier];
        if ([mode isEqualToString:@"count"]) printf("%lu\n", (unsigned long)apps.count);
        else for (NSRunningApplication *app in apps) [app terminate];
        return 0;
    }
    __block BOOL finished = NO; __block int result = 99;
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
        int code = wb_restart_application(identifier, 1.);
        dispatch_async(dispatch_get_main_queue(), ^{ result = code; finished = YES; });
    });
    while (!finished) [NSRunLoop.currentRunLoop runUntilDate:[NSDate dateWithTimeIntervalSinceNow:0.02]];
    printf("%d\n", result); return 0;
} }
''')
        driver = directory / 'driver'
        for source, target, extra in [
            (fixture, executable, []),
            (driver_source, driver, ['-I', str(root / 'native')]),
        ]:
            subprocess.run(['xcrun', 'clang', '-fobjc-arc', '-fblocks', '-framework',
                            'AppKit', *extra, str(source), '-o', str(target)], check=True,
                           capture_output=True, text=True)

        def control(mode, bundle=identifier):
            return int(subprocess.check_output([str(driver), bundle, mode], text=True, timeout=25))

        def wait(predicate):
            deadline = time.monotonic() + 10
            while time.monotonic() < deadline:
                if predicate():
                    return
                time.sleep(.1)
            raise AssertionError('Fixture state did not settle')

        try:
            assert control('restart', identifier + '.missing') == 1
            subprocess.run(['open', '-n', str(app)], check=True, capture_output=True)
            wait(lambda: (resources / 'pid').exists() and control('count') == 1)
            first_pid = (resources / 'pid').read_text()
            code = control('restart')
            assert code == 0, f'running fixture restart failed: {code}'
            wait(lambda: (resources / 'pid').read_text() != first_pid and control('count') == 1)
            restarted_pid = (resources / 'pid').read_text()
            (resources / 'refuse').touch()
            assert control('restart') == 4
            assert control('count') == 1
            assert (resources / 'pid').read_text() == restarted_pid
            (resources / 'refuse').unlink()
            subprocess.run(['open', '-n', str(app)], check=True, capture_output=True)
            wait(lambda: control('count') == 2)
            assert control('restart') == 2
            assert control('count') == 2
            print(json.dumps({'result': 'passed', 'checks': [
                'missing app', 'normal exit then relaunch with a new PID',
                'quit refusal times out without force or duplicate launch',
                'multiple instances remain untouched',
            ]}))
        finally:
            (resources / 'refuse').unlink(missing_ok=True)
            subprocess.run([str(driver), identifier, 'quit'], check=True, capture_output=True, timeout=5)
            wait(lambda: control('count') == 0)


if __name__ == '__main__':
    main()
