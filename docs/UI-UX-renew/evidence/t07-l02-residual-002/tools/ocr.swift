// Vision OCR for the T07-L04 acceptance screenshots.
// usage: ocr PNG [languages]
// Prints one JSON array of {text, confidence, x, y, w, h} in image pixels.
import AppKit
import Foundation
import Vision

let arguments = CommandLine.arguments
guard arguments.count >= 2 else {
    FileHandle.standardError.write("usage: ocr PNG [lang,lang]\n".data(using: .utf8)!)
    exit(2)
}
let path = arguments[1]
let languages = arguments.count >= 3 ? arguments[2].split(separator: ",").map(String.init)
                                      : ["zh-Hans", "en-US"]
guard let image = NSImage(contentsOfFile: path),
      let cgImage = image.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
    FileHandle.standardError.write("cannot read image \(path)\n".data(using: .utf8)!)
    exit(1)
}
let width = Double(cgImage.width)
let height = Double(cgImage.height)
let request = VNRecognizeTextRequest()
request.recognitionLevel = .accurate
request.usesLanguageCorrection = false
request.recognitionLanguages = languages
let handler = VNImageRequestHandler(cgImage: cgImage, options: [:])
do {
    try handler.perform([request])
} catch {
    FileHandle.standardError.write("vision error \(error)\n".data(using: .utf8)!)
    exit(1)
}
var results: [[String: Any]] = []
for observation in request.results ?? [] {
    guard let candidate = observation.topCandidates(1).first else { continue }
    let box = observation.boundingBox
    let x = box.origin.x * width
    let y = (1.0 - box.origin.y - box.size.height) * height
    results.append([
        "text": candidate.string,
        "confidence": Double(candidate.confidence),
        "x": Int(x.rounded()),
        "y": Int(y.rounded()),
        "w": Int((box.size.width * width).rounded()),
        "h": Int((box.size.height * height).rounded()),
    ])
}
let data = try! JSONSerialization.data(withJSONObject: results, options: [.prettyPrinted, .sortedKeys])
print(String(data: data, encoding: .utf8)!)
