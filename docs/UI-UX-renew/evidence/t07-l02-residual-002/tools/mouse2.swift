// Mouse events posted directly to the target process.
// usage: mouse2 click PID X Y
//        mouse2 drag PID X1 Y1 X2 Y2 [steps]
import Cocoa
let a = CommandLine.arguments
guard a.count >= 5 else { print("usage: mouse2 click PID X Y | drag PID X1 Y1 X2 Y2 [steps]"); exit(2) }
let command = a[1]
let pid = pid_t(Int32(a[2])!)
let source = CGEventSource(stateID: .hidSystemState)
func point(_ i: Int) -> CGPoint { CGPoint(x: Double(a[i])!, y: Double(a[i+1])!) }
func post(_ type: CGEventType, _ p: CGPoint) {
    guard let e = CGEvent(mouseEventSource: source, mouseType: type,
                          mouseCursorPosition: p, mouseButton: .left) else { return }
    e.setIntegerValueField(.mouseEventClickState, value: 1)
    e.postToPid(pid)
}
if command == "click" {
    let p = point(3)
    post(.mouseMoved, p); usleep(120_000)
    post(.leftMouseDown, p); usleep(180_000)
    post(.leftMouseUp, p)
    print("mouse2 click pid=\(pid) x=\(p.x) y=\(p.y)")
} else if command == "drag" {
    let p1 = point(3)
    let p2 = point(5)
    let steps = a.count >= 7 ? Int(a[6])! : 30
    post(.mouseMoved, p1); usleep(150_000)
    post(.leftMouseDown, p1); usleep(220_000)
    for i in 1...steps {
        let t = Double(i) / Double(steps)
        let p = CGPoint(x: p1.x + (p2.x - p1.x) * t, y: p1.y + (p2.y - p1.y) * t)
        post(.leftMouseDragged, p)
        usleep(30_000)
    }
    usleep(150_000)
    post(.leftMouseUp, p2)
    print("mouse2 drag pid=\(pid) \(p1) -> \(p2) steps=\(steps)")
} else { exit(2) }
