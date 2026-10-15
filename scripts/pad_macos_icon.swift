// Build the macOS icon canvas without changing the original artwork.
import Foundation
import CoreGraphics
import ImageIO
import UniformTypeIdentifiers

let input = URL(fileURLWithPath: CommandLine.arguments[1])
let output = URL(fileURLWithPath: CommandLine.arguments[2])
let size = 1024
let contentSize = 840 // 82% artwork, with 92 transparent pixels on each side.
guard let source = CGImageSourceCreateWithURL(input as CFURL, nil),
      let image = CGImageSourceCreateImageAtIndex(source, 0, nil),
      let context = CGContext(
        data: nil, width: size, height: size, bitsPerComponent: 8,
        bytesPerRow: size * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!,
        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
      ) else { fatalError("Cannot read icon artwork") }
context.clear(CGRect(x: 0, y: 0, width: size, height: size))
context.interpolationQuality = .high
let margin = (size - contentSize) / 2
context.draw(image, in: CGRect(x: margin, y: margin, width: contentSize, height: contentSize))
guard let result = context.makeImage(),
      let destination = CGImageDestinationCreateWithURL(output as CFURL, UTType.png.identifier as CFString, 1, nil)
else { fatalError("Cannot create padded icon") }
CGImageDestinationAddImage(destination, result, nil)
guard CGImageDestinationFinalize(destination) else { fatalError("Cannot save padded icon") }
