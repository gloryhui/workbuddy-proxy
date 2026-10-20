import ApplicationServices
import Foundation
let a = CommandLine.arguments
guard a.count >= 2, let pid = Int32(a[1]) else { exit(2) }
let app = AXUIElementCreateApplication(pid_t(pid))
func str(_ el: AXUIElement, _ name: String) -> String {
    var v: CFTypeRef?
    if AXUIElementCopyAttributeValue(el, name as CFString, &v) == .success, let s = v {
        return String(describing: s)
    }
    return ""
}
func children(_ el: AXUIElement) -> [AXUIElement] {
    var v: CFTypeRef?
    if AXUIElementCopyAttributeValue(el, kAXChildrenAttribute as CFString, &v) == .success,
       let arr = v as? [AXUIElement] { return arr }
    return []
}
func walk(_ el: AXUIElement, _ depth: Int) -> Bool {
    if depth > 18 { return false }
    var f: CFTypeRef?
    if AXUIElementCopyAttributeValue(el, kAXFocusedAttribute as CFString, &f) == .success,
       let n = f as? NSNumber, n.boolValue {
        print("FOCUS role=\(str(el,kAXRoleAttribute)) title=\(str(el,kAXTitleAttribute)) desc=\(str(el,kAXDescriptionAttribute)) value=\(str(el,kAXValueAttribute))")
        return true
    }
    for c in children(el) { if walk(c, depth + 1) { return true } }
    return false
}
if !walk(app, 0) { print("FOCUS not-exposed") }
