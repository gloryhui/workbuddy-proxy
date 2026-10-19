// Minimal macOS accessibility/screenshot driver for the T07-L04 acceptance run.
//
// Commands:
//   dump PID PATH        write the AX tree of the application (positions included)
//   press PID TITLE      AXPress the first AXButton whose title matches
//   front PID            bring the application to the front
//   scroll PID X Y DY    scroll the wheel at screen point (x, y)
//   click PID X Y        left-click at screen point (x, y)
//   windows PID          print on-screen windows of the process as JSON
//   shot WINDOWID PATH   save a PNG of the window
//   info PID TITLE       print the frame/enabled state of matching buttons
import AppKit
import ApplicationServices
import Foundation

func axAttribute(_ element: AXUIElement, _ name: String) -> CFTypeRef? {
    var value: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element, name as CFString, &value) == .success else {
        return nil
    }
    return value
}

func axString(_ element: AXUIElement, _ name: String) -> String {
    if let value = axAttribute(element, name) {
        if let text = value as? String { return text }
        if let number = value as? NSNumber { return number.stringValue }
        if CFGetTypeID(value) == AXValueGetTypeID() { return "" }
    }
    return ""
}

func axEnabled(_ element: AXUIElement) -> String {
    if let value = axAttribute(element, kAXEnabledAttribute) as? NSNumber {
        return value.boolValue ? "enabled" : "disabled"
    }
    return "enabled"
}

func axFrame(_ element: AXUIElement) -> String {
    var origin = CGPoint.zero
    var size = CGSize.zero
    if let position = axAttribute(element, kAXPositionAttribute),
       CFGetTypeID(position) == AXValueGetTypeID() {
        AXValueGetValue(position as! AXValue, .cgPoint, &origin)
    }
    if let dimension = axAttribute(element, kAXSizeAttribute),
       CFGetTypeID(dimension) == AXValueGetTypeID() {
        AXValueGetValue(dimension as! AXValue, .cgSize, &size)
    }
    return "[\(Int(origin.x)),\(Int(origin.y)) \(Int(size.width))x\(Int(size.height))]"
}

func clean(_ text: String) -> String {
    text.replacingOccurrences(of: "\n", with: "\\n")
        .replacingOccurrences(of: "\r", with: "\\r")
}

func dumpElement(_ element: AXUIElement, depth: Int, into output: inout String, maxDepth: Int = 40) {
    let role = axString(element, kAXRoleAttribute)
    let title = clean(axString(element, kAXTitleAttribute))
    let description = clean(axString(element, kAXDescriptionAttribute))
    let value = clean(axString(element, kAXValueAttribute))
    let prefix = String(repeating: "  ", count: depth)
    output += "\(prefix)\(role) title=\(title) desc=\(description) value=\(value) \(axEnabled(element)) \(axFrame(element))\n"
    guard depth < maxDepth else { return }
    if let children = axAttribute(element, kAXChildrenAttribute) as? [AXUIElement] {
        for child in children {
            dumpElement(child, depth: depth + 1, into: &output, maxDepth: maxDepth)
        }
    }
}

func findButtons(_ element: AXUIElement, title: String, into found: inout [AXUIElement], depth: Int = 0) {
    guard depth < 40 else { return }
    let role = axString(element, kAXRoleAttribute)
    if role == "AXButton" || role == "AXRadioButton" || role == "AXCheckBox" {
        if axString(element, kAXTitleAttribute) == title {
            found.append(element)
        }
    }
    if let children = axAttribute(element, kAXChildrenAttribute) as? [AXUIElement] {
        for child in children {
            findButtons(child, title: title, into: &found, depth: depth + 1)
        }
    }
}

func application(pid: Int32) -> AXUIElement {
    AXUIElementCreateApplication(pid)
}

func windowList(pid: Int32) -> [[String: Any]] {
    let options: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
    guard let raw = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] else {
        return []
    }
    return raw.filter { ($0[kCGWindowOwnerPID as String] as? Int32) == pid }
}

func clickPoint(_ point: CGPoint) {
    let source = CGEventSource(stateID: .hidSystemState)
    let down = CGEvent(mouseEventSource: source, mouseType: .leftMouseDown,
                       mouseCursorPosition: point, mouseButton: .left)
    let up = CGEvent(mouseEventSource: source, mouseType: .leftMouseUp,
                     mouseCursorPosition: point, mouseButton: .left)
    down?.post(tap: .cghidEventTap)
    usleep(60_000)
    up?.post(tap: .cghidEventTap)
}

func scrollPoint(_ point: CGPoint, delta: Int32) {
    let source = CGEventSource(stateID: .hidSystemState)
    let event = CGEvent(scrollWheelEvent2Source: source, units: .pixel,
                        wheelCount: 1, wheel1: delta, wheel2: 0, wheel3: 0)
    event?.location = point
    event?.post(tap: .cghidEventTap)
}

func saveWindow(_ windowID: CGWindowID, to path: String) -> String {
    // CGWindowListCreateImage was obsoleted in macOS 15, so the system
    // screenshot tool (which still accepts a window id) captures the window.
    let process = Process()
    process.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
    process.arguments = ["-l\(windowID)", "-o", "-x", path]
    do {
        try process.run()
        process.waitUntilExit()
        let attributes = try? FileManager.default.attributesOfItem(atPath: path)
        let size = (attributes?[.size] as? Int) ?? 0
        return "screencapture status=\(process.terminationStatus) bytes=\(size)"
    } catch {
        return "screencapture_error \(error)"
    }
}

let arguments = CommandLine.arguments
guard arguments.count >= 3 else {
    FileHandle.standardError.write("usage: axdrive <command> PID|WINDOWID [args]\n".data(using: .utf8)!)
    exit(2)
}
let command = arguments[1]

switch command {
case "dump":
    let pid = Int32(arguments[2])!
    let path = arguments[3]
    var output = ""
    dumpElement(application(pid: pid), depth: 0, into: &output)
    try? output.write(toFile: path, atomically: true, encoding: .utf8)
    print("dump bytes=\(output.utf8.count) path=\(path)")
case "press":
    let pid = Int32(arguments[2])!
    let title = arguments[3]
    var found: [AXUIElement] = []
    findButtons(application(pid: pid), title: title, into: &found)
    guard !found.isEmpty else {
        print("press NOTFOUND title=\(title)")
        exit(1)
    }
    var pressed = 0
    for button in found {
        if AXUIElementPerformAction(button, kAXPressAction as CFString) == .success {
            pressed += 1
        }
    }
    print("press title=\(title) matches=\(found.count) pressed=\(pressed) frame=\(axFrame(found[0]))")
case "front":
    let pid = Int32(arguments[2])!
    if let app = NSRunningApplication(processIdentifier: pid) {
        app.activate()
        print("front ok pid=\(pid)")
    } else {
        print("front NOTFOUND pid=\(pid)")
        exit(1)
    }
case "scroll":
    let x = Double(arguments[2])!
    let y = Double(arguments[3])!
    let delta = Int32(arguments[4])!
    scrollPoint(CGPoint(x: x, y: y), delta: delta)
    print("scroll x=\(x) y=\(y) dy=\(delta)")
case "click":
    let x = Double(arguments[2])!
    let y = Double(arguments[3])!
    clickPoint(CGPoint(x: x, y: y))
    print("click x=\(x) y=\(y)")
case "windows":
    let pid = Int32(arguments[2])!
    let windows = windowList(pid: pid).map { window -> [String: Any] in
        [
            "id": window[kCGWindowNumber as String] ?? -1,
            "name": window[kCGWindowName as String] ?? "",
            "bounds": window[kCGWindowBounds as String] ?? [:],
            "layer": window[kCGWindowLayer as String] ?? -1,
        ]
    }
    let data = try! JSONSerialization.data(withJSONObject: windows, options: [.prettyPrinted, .sortedKeys])
    print(String(data: data, encoding: .utf8)!)
case "info":
    let pid = Int32(arguments[2])!
    let title = arguments[3]
    var found: [AXUIElement] = []
    findButtons(application(pid: pid), title: title, into: &found)
    for button in found {
        print("title=\(title) \(axEnabled(button)) \(axFrame(button))")
    }
    print("matches=\(found.count)")
case "shot":
    let windowID = CGWindowID(UInt32(arguments[2])!)
    let path = arguments[3]
    print(saveWindow(windowID, to: path))
default:
    FileHandle.standardError.write("unknown command \(command)\n".data(using: .utf8)!)
    exit(2)
}
