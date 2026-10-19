import Cocoa
let a = CommandLine.arguments
guard a.count >= 4 else { print("usage: hscroll X Y DX"); exit(2) }
let x = Double(a[1])!, y = Double(a[2])!, dx = Int32(a[3])!
let source = CGEventSource(stateID: .hidSystemState)
let event = CGEvent(scrollWheelEvent2Source: source, units: .pixel,
                    wheelCount: 2, wheel1: 0, wheel2: dx, wheel3: 0)
event?.location = CGPoint(x: x, y: y)
event?.post(tap: .cghidEventTap)
print("hscroll x=\(x) y=\(y) dx=\(dx)")
