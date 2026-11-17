import CoreGraphics
import Foundation
import ImageIO

// Run from the repository root: swift scripts/generate-portrait-app-icons.swift
let files = FileManager.default
let root = files.currentDirectoryPath
let desktop = "\(root)/build/desktop/icons"
let web = "\(root)/talos-app/web/public/app-icons"
let sources = "\(root)/build/brand/app-icons/portrait"
let classic = "\(root)/build/brand/talos-app-icon.svg"
let scratch = files.temporaryDirectory.appendingPathComponent("talos-portrait-\(UUID())")
let colorSpace = CGColorSpace(name: CGColorSpace.sRGB)!

func run(_ tool: String, _ arguments: [String]) throws {
    let process = Process()
    process.executableURL = URL(fileURLWithPath: "/usr/bin/env")
    process.arguments = [tool] + arguments
    process.standardOutput = FileHandle.nullDevice
    try process.run()
    process.waitUntilExit()
    precondition(process.terminationStatus == 0, "\(tool) failed with status \(process.terminationStatus)")
}

func capture(_ pattern: String, in text: String) -> String {
    let expression = try! NSRegularExpression(pattern: pattern)
    let match = expression.firstMatch(in: text, range: NSRange(text.startIndex..., in: text))!
    return String(text[Range(match.range(at: 1), in: text)!])
}

func readPixels(_ url: URL, size: Int) -> [UInt8] {
    let source = CGImageSourceCreateWithURL(url as CFURL, nil)!
    let image = CGImageSourceCreateImageAtIndex(source, 0, nil)!
    precondition(image.width == size && image.height == size, "Unexpected dimensions in \(url)")
    var pixels = [UInt8](repeating: 0, count: size * size * 4)
    pixels.withUnsafeMutableBytes { buffer in
        let context = CGContext(
            data: buffer.baseAddress, width: size, height: size, bitsPerComponent: 8,
            bytesPerRow: size * 4, space: colorSpace,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        )!
        context.setBlendMode(.copy)
        context.draw(image, in: CGRect(x: 0, y: 0, width: size, height: size))
    }
    return pixels
}

func writePortrait(artwork: [UInt8], classic: [UInt8], size: Int, destination: URL) {
    var pixels = [UInt8](repeating: 0, count: artwork.count)
    for offset in stride(from: 0, to: pixels.count, by: 4) {
        let alpha = classic[offset + 3]
        guard alpha != 0 else { continue }
        let artworkAlpha = Int(artwork[offset + 3])
        precondition(artworkAlpha > 0, "Portrait does not cover the full app icon tile")
        pixels[offset + 3] = alpha
        for channel in 0..<3 {
            // Copy the fitted artwork's unassociated color under the exact Talos alpha.
            let premultiplied = Int(artwork[offset + channel])
            pixels[offset + channel] = UInt8((premultiplied * 255 + artworkAlpha / 2) / artworkAlpha)
        }
    }
    let provider = CGDataProvider(data: Data(pixels) as CFData)!
    let image = CGImage(
        width: size, height: size, bitsPerComponent: 8, bitsPerPixel: 32,
        bytesPerRow: size * 4, space: colorSpace,
        bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
        provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent
    )!
    let png = CGImageDestinationCreateWithURL(destination as CFURL, "public.png" as CFString, 1, nil)!
    CGImageDestinationAddImage(png, image, nil)
    precondition(CGImageDestinationFinalize(png), "Failed to write \(destination)")
}

func generate() throws {
    try files.createDirectory(at: scratch, withIntermediateDirectories: false)
    defer { try? files.removeItem(at: scratch) }

    let classicSVG = try String(contentsOfFile: classic, encoding: .utf8)
    let viewBox = capture(#"viewBox="([^"]+)""#, in: classicSVG)
    let tile = capture(#"(<rect\b[^>]*\/>)"#, in: classicSVG)
    func dimension(_ name: String) -> Double {
        Double(capture("\\s\(name)=\"([^\"]+)\"", in: tile))!
    }
    let halfStroke = dimension("stroke-width") / 2
    let x = dimension("x") - halfStroke
    let y = dimension("y") - halfStroke
    let width = dimension("width") + 2 * halfStroke
    let height = dimension("height") + 2 * halfStroke

    let master = scratch.appendingPathComponent("source.png")
    let fitted = scratch.appendingPathComponent("fitted.svg")
    let webSizes = [96: "favicon.png", 180: "apple-touch-icon.png", 256: "icon.png"]
    let sizes = ([1024] + webSizes.keys).sorted()
    let artwork = scratch.appendingPathComponent("artwork.png")
    let mask = scratch.appendingPathComponent("mask.png")

    for (source, preset) in [("dark", "portrait"), ("light", "portrait-light")] {
        let webOutput = "\(web)/\(preset)"
        try files.createDirectory(atPath: webOutput, withIntermediateDirectories: true)
        try run("sips", ["-s", "format", "png", "\(sources)/\(source).jpg", "--out", master.path])
        let sourcePNG = try Data(contentsOf: master).base64EncodedString()
        let fittedSVG = """
        <svg xmlns="http://www.w3.org/2000/svg" viewBox="\(viewBox)">
          <image x="\(x)" y="\(y)" width="\(width)" height="\(height)" href="data:image/png;base64,\(sourcePNG)"/>
        </svg>
        """
        try fittedSVG.write(to: fitted, atomically: true, encoding: .utf8)

        for size in sizes {
            let flags = ["-w", String(size), "-h", String(size)]
            try run("rsvg-convert", flags + ["-o", artwork.path, fitted.path])
            try run("rsvg-convert", flags + ["-o", mask.path, classic])
            let export = scratch.appendingPathComponent("\(preset)-\(size).png")
            writePortrait(artwork: readPixels(artwork, size: size), classic: readPixels(mask, size: size),
                          size: size, destination: export)
            if let name = webSizes[size] {
                try Data(contentsOf: export).write(to: URL(fileURLWithPath: "\(webOutput)/\(name)"), options: .atomic)
            }
            if size == 1024 {
                try Data(contentsOf: export).write(to: URL(fileURLWithPath: "\(desktop)/\(preset).png"), options: .atomic)
            }
        }
    }
}

try generate()
