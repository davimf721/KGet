#!/usr/bin/swift
//
// generate-dmg-background.swift
// Generates a beautiful liquid glass-inspired DMG background for KGet.
// Usage: swift generate-dmg-background.swift <output.png>
//
// Uses NSImage.lockFocus so all drawing is in natural screen coordinates
// (origin top-left, y increases downward) — no flipping issues.
//

import AppKit

let W: CGFloat = 720
let H: CGFloat = 440

guard CommandLine.arguments.count >= 2 else {
    fputs("Usage: swift generate-dmg-background.swift <output.png>\n", stderr)
    exit(1)
}
let outputPath = CommandLine.arguments[1]

// We need an NSApplication for NSGraphicsContext
let _ = NSApplication.shared

let image = NSImage(size: NSSize(width: W, height: H))
image.lockFocus()

guard let ctx = NSGraphicsContext.current?.cgContext else {
    image.unlockFocus()
    fputs("Failed to get CGContext\n", stderr)
    exit(1)
}

let cs = CGColorSpaceCreateDeviceRGB()

// ── Background gradient ────────────────────────────────────────────────────
// Navy → deep indigo, top-to-bottom in screen coords (y=0 is top here)
let bgColors: [CGColor] = [
    CGColor(red: 0.05, green: 0.06, blue: 0.14, alpha: 1),
    CGColor(red: 0.08, green: 0.07, blue: 0.20, alpha: 1),
    CGColor(red: 0.06, green: 0.10, blue: 0.22, alpha: 1),
]
let bgGrad = CGGradient(colorsSpace: cs, colors: bgColors as CFArray, locations: [0, 0.5, 1])!
// In lockFocus, NSGraphicsContext uses flipped (y=0 top) for high-level drawing,
// but CGContext still has origin bottom-left. Adjust start/end:
ctx.drawLinearGradient(bgGrad,
    start: CGPoint(x: 0, y: H),   // top of image in CG coords
    end:   CGPoint(x: W, y: 0),   // bottom of image in CG coords
    options: [])

// ── Dot noise texture ──────────────────────────────────────────────────────
for row in stride(from: 0, to: Int(H), by: 24) {
    for col in stride(from: 0, to: Int(W), by: 24) {
        let alpha = CGFloat.random(in: 0.015...0.04)
        ctx.setFillColor(CGColor(red: 1, green: 1, blue: 1, alpha: alpha))
        ctx.fillEllipse(in: CGRect(x: CGFloat(col) + 0.5, y: CGFloat(row) + 0.5, width: 1, height: 1))
    }
}

// ── Radial glow orbs ───────────────────────────────────────────────────────
func drawOrb(cx: CGFloat, cy: CGFloat, radius: CGFloat, r: CGFloat, g: CGFloat, b: CGFloat, alpha: CGFloat) {
    let colors: [CGColor] = [
        CGColor(red: r, green: g, blue: b, alpha: alpha),
        CGColor(red: r, green: g, blue: b, alpha: 0),
    ]
    let grad = CGGradient(colorsSpace: cs, colors: colors as CFArray, locations: [0, 1] as [CGFloat])!
    ctx.drawRadialGradient(grad,
        startCenter: CGPoint(x: cx, y: cy), startRadius: 0,
        endCenter:   CGPoint(x: cx, y: cy), endRadius: radius,
        options: [.drawsBeforeStartLocation, .drawsAfterEndLocation])
}

// Blue orb — app zone (left), cy in CG coords = H - screen_y
drawOrb(cx: 190, cy: H/2, radius: 210, r: 0.15, g: 0.35, b: 1.0, alpha: 0.22)
// Purple orb — Applications zone (right)
drawOrb(cx: 530, cy: H/2, radius: 190, r: 0.55, g: 0.20, b: 0.95, alpha: 0.18)
// Center accent
drawOrb(cx: 360, cy: H/2, radius: 160, r: 0.20, g: 0.50, b: 1.0, alpha: 0.07)

// ── Glass panel ────────────────────────────────────────────────────────────
let panelRect = CGRect(x: 36, y: 56, width: W - 72, height: H - 112)
let panelRadius: CGFloat = 24
let panelPath = CGPath(roundedRect: panelRect, cornerWidth: panelRadius, cornerHeight: panelRadius, transform: nil)

ctx.saveGState()
ctx.addPath(panelPath)
ctx.clip()
let glassColors: [CGColor] = [
    CGColor(red: 1, green: 1, blue: 1, alpha: 0.07),
    CGColor(red: 1, green: 1, blue: 1, alpha: 0.02),
]
let glassGrad = CGGradient(colorsSpace: cs, colors: glassColors as CFArray, locations: [0, 1] as [CGFloat])!
ctx.drawLinearGradient(glassGrad,
    start: CGPoint(x: panelRect.midX, y: panelRect.maxY),
    end:   CGPoint(x: panelRect.midX, y: panelRect.minY),
    options: [])
ctx.restoreGState()

// Panel border
ctx.saveGState()
ctx.addPath(panelPath)
ctx.setStrokeColor(CGColor(red: 1, green: 1, blue: 1, alpha: 0.13))
ctx.setLineWidth(0.75)
ctx.strokePath()
ctx.restoreGState()

// Top specular highlight on glass panel
ctx.saveGState()
let hlRect = CGRect(x: 38, y: panelRect.maxY - 42, width: W - 76, height: 42)
let hlPath = CGPath(roundedRect: hlRect, cornerWidth: 22, cornerHeight: 22, transform: nil)
ctx.addPath(hlPath)
ctx.clip()
let hlColors: [CGColor] = [
    CGColor(red: 1, green: 1, blue: 1, alpha: 0.20),
    CGColor(red: 1, green: 1, blue: 1, alpha: 0),
]
let hlGrad = CGGradient(colorsSpace: cs, colors: hlColors as CFArray, locations: [0, 1] as [CGFloat])!
ctx.drawLinearGradient(hlGrad,
    start: CGPoint(x: hlRect.midX, y: hlRect.maxY),
    end:   CGPoint(x: hlRect.midX, y: hlRect.minY),
    options: [])
ctx.restoreGState()

// ── Centre divider ─────────────────────────────────────────────────────────
let divX: CGFloat = 362
let divColors: [CGColor] = [
    CGColor(red: 1, green: 1, blue: 1, alpha: 0),
    CGColor(red: 1, green: 1, blue: 1, alpha: 0.22),
    CGColor(red: 1, green: 1, blue: 1, alpha: 0),
]
let divGrad = CGGradient(colorsSpace: cs, colors: divColors as CFArray, locations: [0, 0.5, 1] as [CGFloat])!
ctx.drawLinearGradient(divGrad,
    start: CGPoint(x: divX, y: 76),
    end:   CGPoint(x: divX, y: H - 76),
    options: [])

// ── Arrow ──────────────────────────────────────────────────────────────────
func drawArrow(cx: CGFloat, cy: CGFloat, size: CGFloat) {
    let arrowColor = CGColor(red: 1, green: 1, blue: 1, alpha: 0.65)
    ctx.setStrokeColor(arrowColor)
    ctx.setLineWidth(2.5)
    ctx.setLineCap(.round)
    ctx.setLineJoin(.round)

    // Shaft
    ctx.beginPath()
    ctx.move(to:    CGPoint(x: cx - size * 0.36, y: cy))
    ctx.addLine(to: CGPoint(x: cx + size * 0.18, y: cy))
    ctx.strokePath()

    // Arrowhead
    ctx.beginPath()
    ctx.move(to:    CGPoint(x: cx + size * 0.18, y: cy - size * 0.22))
    ctx.addLine(to: CGPoint(x: cx + size * 0.42, y: cy))
    ctx.addLine(to: CGPoint(x: cx + size * 0.18, y: cy + size * 0.22))
    ctx.strokePath()
}
drawArrow(cx: divX, cy: H / 2, size: 28)

// ── Text labels — use NSAttributedString.draw() so origin is top-left ──────
// NSImage.lockFocus gives us a flipped context for NSAttributedString,
// so y=0 is the TOP of the image here (correct for screen coords).

func label(_ text: String, x: CGFloat, y: CGFloat, size: CGFloat,
           weight: NSFont.Weight = .regular, alpha: CGFloat = 0.7, centered: Bool = false) {
    let font = NSFont.systemFont(ofSize: size, weight: weight)
    let color = NSColor.white.withAlphaComponent(alpha)
    let attrs: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: color]
    let str = NSAttributedString(string: text, attributes: attrs)
    let strSize = str.size()
    let drawPoint = NSPoint(x: centered ? x - strSize.width / 2 : x, y: y)
    str.draw(at: drawPoint)
}

// App label — top-left zone (y near H = near top in y-up coords)
label("KGet",              x: 190, y: H - 90,  size: 20, weight: .bold,   alpha: 0.90, centered: true)
label("Download Manager",  x: 190, y: H - 112, size: 11, weight: .regular, alpha: 0.42, centered: true)

// Applications label — top-right zone
label("Applications",      x: 530, y: H - 96,  size: 13, weight: .medium, alpha: 0.58, centered: true)

// Bottom instruction (near bottom of window, below icons)
label("Drag KGet to Applications to install",
      x: W/2, y: 26, size: 11, weight: .regular, alpha: 0.35, centered: true)

// Tiny version watermark near top
label("kget.app", x: W/2, y: H - 18, size: 9, weight: .regular, alpha: 0.18, centered: true)

// ── Export PNG ─────────────────────────────────────────────────────────────
image.unlockFocus()

guard let tiff = image.tiffRepresentation,
      let rep  = NSBitmapImageRep(data: tiff),
      let png  = rep.representation(using: .png, properties: [:]) else {
    fputs("Failed to encode PNG\n", stderr)
    exit(1)
}

try! png.write(to: URL(fileURLWithPath: outputPath))
print("Generated: \(outputPath) (\(Int(W))×\(Int(H)))")
