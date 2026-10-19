import Cocoa
let a = CommandLine.arguments
guard a.count >= 2 else { exit(2) }
let pid = pid_t(Int32(a[1])!)
if let app = NSRunningApplication(processIdentifier: pid) {
    let ok = app.activate(options: [.activateAllWindows, .activateIgnoringOtherApps])
    print("activate=\(ok) active=\(app.isActive) hidden=\(app.isHidden)")
} else { print("not found") }
if let front = NSWorkspace.shared.frontmostApplication {
    print("frontmost=\(front.localizedName ?? "?") pid=\(front.processIdentifier)")
}
