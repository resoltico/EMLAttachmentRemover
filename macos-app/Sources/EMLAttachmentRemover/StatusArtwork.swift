// Copyright (c) 2026 Ervins Strauhmanis.
// SPDX-License-Identifier: LicenseRef-Proprietary-Artwork
// Original geometric artwork; see retained artwork documents and root LICENSE.
import SwiftUI

/// Fresh Canvas rendering of the retained 100-unit envelope/badge geometry.
/// Adjacent status text supplies accessibility meaning; this drawing is decorative.
struct StatusArtwork: View {
  enum Kind { case identity, processing, success, attention, stopped }
  enum Tone { case accent, success, warning, failure, neutral }
  let kind: Kind
  var tone: Tone = .accent
  @Environment(\.colorScheme) private var scheme
  @Environment(\.colorSchemeContrast) private var contrast

  var body: some View {
    Canvas { original, size in
      guard size.width > 0, size.height > 0 else { return }
      var context = original
      context.translateBy(x: 0, y: size.height)
      context.scaleBy(x: size.width / 100, y: -size.height / 100)
      let dark = scheme == .dark
      let high = contrast == .increased
      let index = (dark ? 1 : 0) + (high ? 2 : 0)
      let compact: CGFloat = min(size.width, size.height) < 24 ? 1 : 0
      let envelopeWidth: CGFloat = 8 + compact + (high ? 1.5 : 0)
      let markWidth: CGFloat = 5.5 + compact + (high ? 1 : 0)
      let ringWidth: CGFloat = 7 + (high ? 0.5 : 0)
      let envelope = color([0x636366, 0xAEAEB2, 0x3A3A3C, 0xE5E5EA], index)
      let mark = color([0xFFFFFF, 0x1C1C1E, 0xFFFFFF, 0x000000], index)
      var clipped = context
      var cutout = Path(CGRect(x: 0, y: 0, width: 100, height: 100))
      cutout.addEllipse(in: CGRect(x: 47, y: 5, width: 50, height: 50))
      clipped.clip(to: cutout, style: FillStyle(eoFill: true))
      clipped.stroke(
        Path(roundedRect: CGRect(x: 8, y: 34, width: 62, height: 46), cornerRadius: 9),
        with: .color(envelope), style: stroke(envelopeWidth))
      line([(15, 73), (39, 54), (63, 73)], in: &clipped, color: envelope, width: envelopeWidth)
      let badge = Path(ellipseIn: CGRect(x: 54, y: 12, width: 36, height: 36))
      switch kind {
      case .identity:
        let fill = color([0x0B3D91, 0xFFD166, 0x0B3D91, 0xFFD166], index)
        let ink = color([0xFFD166, 0x0B3D91, 0xFFD166, 0x0B3D91], index)
        context.fill(badge, with: .color(fill))
        line([(63.5, 21.5), (68.5, 26.5)], in: &context, color: ink, width: markWidth)
        line([(75.5, 33.5), (80.5, 38.5)], in: &context, color: ink, width: markWidth)
      case .processing:
        let ink = badgeColor(index)
        let radius = 18 - ringWidth / 2
        let track = Path(
          ellipseIn: CGRect(x: 72 - radius, y: 30 - radius, width: radius * 2, height: radius * 2))
        context.stroke(
          track, with: .color(ink.opacity(high ? 0.45 : 0.3)), style: stroke(ringWidth))
        var arc = Path()
        arc.addArc(
          center: CGPoint(x: 72, y: 30), radius: radius, startAngle: .degrees(90),
          endAngle: .degrees(-139), clockwise: true)
        context.stroke(arc, with: .color(ink), style: stroke(ringWidth))
      case .success:
        context.fill(badge, with: .color(badgeColor(index)))
        line([(63, 30), (69, 24), (81, 37)], in: &context, color: mark, width: markWidth)
      case .attention:
        context.fill(badge, with: .color(badgeColor(index)))
        let width: CGFloat = 5 + compact * 1.5
        context.fill(
          Path(
            roundedRect: CGRect(x: 72 - width / 2, y: 28, width: width, height: 14), cornerRadius: 2
          ), with: .color(mark))
        let radius: CGFloat = 3 + compact * 0.6
        context.fill(
          Path(
            ellipseIn: CGRect(x: 72 - radius, y: 22 - radius, width: radius * 2, height: radius * 2)
          ), with: .color(mark))
      case .stopped:
        context.fill(badge, with: .color(badgeColor(index)))
        context.fill(
          Path(roundedRect: CGRect(x: 65, y: 23, width: 14, height: 14), cornerRadius: 3),
          with: .color(mark))
      }
    }
    .accessibilityHidden(true)
  }

  private func color(_ values: [UInt32], _ index: Int) -> Color {
    let value = values[index]
    return Color(
      .sRGB, red: Double((value >> 16) & 255) / 255,
      green: Double((value >> 8) & 255) / 255, blue: Double(value & 255) / 255, opacity: 1)
  }
  private func badgeColor(_ index: Int) -> Color {
    switch tone {
    case .accent: color([0x0066D6, 0x409CFF, 0x0050A8, 0x6BB3FF], index)
    case .success: color([0x1F7A35, 0x30D158, 0x146128, 0x5BE07A], index)
    case .warning: color([0xB54700, 0xFF9F0A, 0x8F3800, 0xFFB340], index)
    case .failure: color([0xC4281C, 0xFF6961, 0xA11A12, 0xFF8A80], index)
    case .neutral: color([0x636366, 0xAEAEB2, 0x48484A, 0xD1D1D6], index)
    }
  }
  private func stroke(_ width: CGFloat) -> StrokeStyle {
    StrokeStyle(lineWidth: width, lineCap: .round, lineJoin: .round)
  }
  private func line(
    _ points: [(CGFloat, CGFloat)], in context: inout GraphicsContext, color: Color, width: CGFloat
  ) {
    var path = Path()
    guard let first = points.first else { return }
    path.move(to: CGPoint(x: first.0, y: first.1))
    for point in points.dropFirst() { path.addLine(to: CGPoint(x: point.0, y: point.1)) }
    context.stroke(path, with: .color(color), style: stroke(width))
  }
}
