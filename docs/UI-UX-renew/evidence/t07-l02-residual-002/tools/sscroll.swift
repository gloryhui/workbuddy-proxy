import Cocoa
let a = CommandLine.arguments
guard a.count >= 4 else { print("usage: sscroll X Y DY"); exit(2) }
let x = Double(a[1])!, y = Double(a[2])!, dy = Int32(a[3])!
let source = CGEventSource(stateID: .hidSystemState)
if let e = CGEvent(scrollWheelEvent2Source: source, units: .pixel, wheelCount: 1,
                   wheel1: dy, wheel2: 0, wheel3: 0) {
    e.flags = .maskShift
    e.location = CGPoint(x: x, y: y)
    e.post(tap: .cghidEventTap)
}
print("sscroll x=\(x) y=\(y) dy=\(dy)")
