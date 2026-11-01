import AppKit
import Foundation
let pid: pid_t = 32975
let front = NSWorkspace.shared.frontmostApplication
let app = NSRunningApplication(processIdentifier: pid)
let now = ISO8601DateFormatter().string(from: Date())
let result: [String: Any] = ["timestamp_utc": now, "target_pid": Int(pid), "frontmost_pid": front.map { Int($0.processIdentifier) } as Any? ?? NSNull(), "target_found": app != nil, "target_is_active": app.map { $0.isActive } as Any? ?? NSNull(), "frontmost_is_target": front.map { $0.processIdentifier == pid } as Any? ?? NSNull()]
let data = try JSONSerialization.data(withJSONObject: result, options: [.prettyPrinted, .sortedKeys])
print(String(decoding: data, as: UTF8.self))
