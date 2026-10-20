import AppKit
import Foundation
// Usage: swift scripts/extract-design-header.swift <original @2x.png> <output-dir>
// Run only when updating the bundled design assets; not required to build the app.
guard CommandLine.arguments.count == 3 else {
    fatalError("Expected original PNG path and output directory")
}
let input=CommandLine.arguments[1]
let output=CommandLine.arguments[2]
let b=NSBitmapImageRep(data:try Data(contentsOf:URL(fileURLWithPath:input)))!
func pixel(_ x:Int,_ y:Int)->[Double] {let c=b.colorAt(x:x,y:y)!.usingColorSpace(.sRGB)!;return [c.redComponent,c.greenComponent,c.blueComponent]}
func linear(_ c:Double)->Double {c<=0.04045 ? c/12.92 : pow((c+0.055)/1.055,2.4)}
let dark=[92.0,102.0,121.0].map{$0/255}
let white=[1.0,1.0,1.0]
let page=[247.0,248.0,249.0].map{$0/255}
let segment=[241.0,242.0,245.0].map{$0/255}
let green=[0.0,178.0,156.0].map{$0/255}
func extract(_ name:String,_ x:Int,_ y:Int,_ foreground:[Double],_ background:[Double],_ paint:[Double]) throws {
 let out=NSBitmapImageRep(bitmapDataPlanes:nil,pixelsWide:32,pixelsHigh:32,bitsPerSample:8,samplesPerPixel:4,hasAlpha:true,isPlanar:false,colorSpaceName:.deviceRGB,bitmapFormat:.alphaNonpremultiplied,bytesPerRow:128,bitsPerPixel:32)!
 let fg=foreground.map(linear),bg=background.map(linear)
 let denominator=zip(fg,bg).reduce(0){$0+pow($1.0-$1.1,2)}
 for yy in 0..<32 {for xx in 0..<32 {
  let p=pixel(x+xx,y+yy).map(linear)
  var numerator = 0.0
  for channel in 0..<3 { numerator += (p[channel]-bg[channel])*(fg[channel]-bg[channel]) }
  let a = max(0.0,min(1.0,numerator/denominator))
  out.setColor(NSColor(deviceRed:paint[0],green:paint[1],blue:paint[2],alpha:a),atX:xx,y:yy)
 }}
 try out.representation(using:.png,properties:[:])!.write(to:URL(fileURLWithPath:output+"/"+name+".png"))
}
for (name,x) in [("power",2124),("refresh",2192),("globe",2260),("fullscreen",2328),("github",2396),("user",2464)] {try extract("header-"+name,x,32,dark,page,dark)}
for (name,x,y,fg,bg) in [("all",1893,161,white,green),("builtin",2099,161,dark,segment),("custom",2305,161,dark,segment)] {
 try extract("source-"+name,x,y,fg,bg,dark)
 try extract("source-"+name+"-selected",x,y,fg,bg,white)
}
print("Extracted 12 transparent 32×32 PNGs (render at 16 logical pixels).")
