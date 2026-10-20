import ApplicationServices
import Foundation
let a = CommandLine.arguments
guard a.count >= 2, let pid = Int32(a[1]) else { print("usage: focusinfo PID"); exit(2) }
let app = AXUIElementCreateApplication(pid_t(pid))
var value: CFTypeRef?
let err = AXUIElementCopyAttributeValue(app, kAXFocusedUIElementAttribute as CFString, &value)
if err != .success { print("focused=none err=\(err.rawValue)"); exit(0) }
let el = value as! AXUIElement
func attr(_ name: String) -> String {
    var v: CFTypeRef?
    if AXUIElementCopyAttributeValue(el, name as CFString, &v) == .success, let s = v {
        return String(describing: s)
    }
    return ""
}
print("role=\(attr(kAXRoleAttribute)) title=\(attr(kAXTitleAttribute)) desc=\(attr(kAXDescriptionAttribute)) value=\(attr(kAXValueAttribute))")
