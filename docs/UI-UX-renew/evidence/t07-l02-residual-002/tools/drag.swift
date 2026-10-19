// Mouse drag for column-border resize in the request log.
// usage: drag X1 Y1 X2 Y2 [steps]
import Cocoa
let a = CommandLine.arguments
guard a.count >= 5 else { print("usage: drag X1 Y1 X2 Y2 [steps]"); exit(2) }
let x1 = Double(a[1])!, y1 = Double(a[2])!, x2 = Double(a[3])!, y2 = Double(a[4])!
let steps = a.count >= 6 ? Int(a[5])! : 24
let source = CGEventSource(stateID: .hidSystemState)
let start = CGPoint(x: x1, y: y1)
CGEvent(mouseEventSource: source, mouseType: .mouseMoved,
        mouseCursorPosition: start, mouseButton: .left)?.post(tap: .cghidEventTap)
usleep(120_000)
CGEvent(mouseEventSource: source, mouseType: .leftMouseDown,
        mouseCursorPosition: start, mouseButton: .left)?.post(tap: .cghidEventTap)
usleep(160_000)
for i in 1...steps {
    let t = Double(i) / Double(steps)
    let p = CGPoint(x: x1 + (x2 - x1) * t, y: y1 + (y2 - y1) * t)
    CGEvent(mouseEventSource: source, mouseType: .leftMouseDragged,
            mouseCursorPosition: p, mouseButton: .left)?.post(tap: .cghidEventTap)
    usleep(25_000)
}
usleep(120_000)
CGEvent(mouseEventSource: source, mouseType: .leftMouseUp,
        mouseCursorPosition: CGPoint(x: x2, y: y2), mouseButton: .left)?.post(tap: .cghidEventTap)
print("drag \(x1),\(y1) -> \(x2),\(y2) steps=\(steps)")
