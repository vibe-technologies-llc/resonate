import QuartzCore
import ResonateLogic
import SwiftUI
import UIKit

struct PlayingBarsLayers: UIViewRepresentable {
    let features: Features
    let following: Bool
    let calm: Bool
    let ink: Color

    func makeUIView(context: Context) -> PlayingBarsView {
        PlayingBarsView(frame: .zero)
    }

    func updateUIView(_ view: PlayingBarsView, context: Context) {
        view.ink(ink.resolve(in: context.environment).cgColor)
        view.follow(following ? features : nil, calm: calm)
    }

    static func dismantleUIView(_ view: PlayingBarsView, coordinator: ()) {
        view.leave()
    }
}

final class PlayingBarsView: UIView {
    private static let heardAt = CAFrameRateRange(minimum: 20, maximum: 20, preferred: 20)
    private static let flowsOver: CFTimeInterval = 0.05
    private static let settlesOver: CFTimeInterval = 0.3
    private static let heightKey = "height"
    private static let unmoved: CGFloat = 0.01
    private static let unanimated: [String: any CAAction] = [
        "bounds": NSNull(),
        "position": NSNull(),
        "backgroundColor": NSNull(),
        "cornerRadius": NSNull(),
    ]

    private enum Pace {
        case atOnce
        case flowing
        case settling
    }

    private let bars = (0..<HeardBands.count).map { _ in CALayer() }
    private var heard = PlayingBars()
    private var link: CADisplayLink?
    private var followed: Features?
    private var listening: Features?
    private var calm = false
    private var shape: PlayingBarsShape?
    private var laidIn = CGRect.null
    private var inked: CGColor?

    override init(frame: CGRect) {
        super.init(frame: frame)
        isUserInteractionEnabled = false
        isOpaque = false
        backgroundColor = .clear
        isAccessibilityElement = false
        for bar in bars {
            bar.anchorPoint = CGPoint(x: 0.5, y: 1)
            bar.actions = Self.unanimated
            layer.addSublayer(bar)
        }
    }

    required init?(coder: NSCoder) {
        return nil
    }

    func ink(_ colour: CGColor) {
        guard inked.map({ $0 != colour }) ?? true else { return }
        inked = colour
        for bar in bars { bar.backgroundColor = colour }
    }

    func follow(_ features: Features?, calm: Bool) {
        self.calm = calm
        followed = features
        settle()
    }

    func leave() {
        followed = nil
        stop()
    }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        settle()
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        guard bounds != laidIn else { return }
        laidIn = bounds

        let laid = PlayingBarsShape(height: bounds.height)
        let left = bounds.midX - laid.width / 2
        shape = laid
        for (index, bar) in bars.enumerated() {
            bar.removeAnimation(forKey: Self.heightKey)
            bar.cornerRadius = laid.barWidth / 2
            bar.position = CGPoint(x: left + laid.middle(of: index), y: bounds.maxY)
            bar.bounds = CGRect(x: 0, y: 0, width: laid.barWidth, height: laid.barHeight(heard.heights[index]))
        }
    }

    private func settle() {
        guard window != nil, let features = followed else {
            rest()
            return
        }
        guard link == nil else { return }

        let started = CADisplayLink(target: self, selector: #selector(tick))
        started.preferredFrameRateRange = Self.heardAt
        started.add(to: .main, forMode: .common)
        features.hearLoudness(true)
        listening = features
        link = started
    }

    private func rest() {
        let wasFollowing = link != nil
        stop()
        guard window != nil else { return }

        heard.rest()
        show(wasFollowing && !calm ? .settling : .atOnce)
    }

    private func stop() {
        link?.invalidate()
        link = nil
        listening?.hearLoudness(false)
        listening = nil
    }

    @objc private func tick(_ link: CADisplayLink) {
        guard let listening else { return }
        heard.hear(listening.heardLevel.bands)
        show(.flowing)
    }

    private func show(_ pace: Pace) {
        guard let shape else { return }
        for (bar, share) in zip(bars, heard.heights) {
            let target = shape.barHeight(share)
            if pace == .atOnce, abs(bar.bounds.height - target) <= Self.unmoved { continue }

            let from = bar.presentation()?.bounds.height ?? bar.bounds.height
            bar.removeAnimation(forKey: Self.heightKey)
            bar.bounds.size.height = target
            guard let moving = Self.timed(pace), abs(from - target) > Self.unmoved else { continue }

            moving.fromValue = from
            moving.toValue = target
            bar.add(moving, forKey: Self.heightKey)
        }
    }

    private static func timed(_ pace: Pace) -> CABasicAnimation? {
        let animation = CABasicAnimation(keyPath: "bounds.size.height")
        switch pace {
        case .atOnce:
            return nil
        case .flowing:
            animation.duration = flowsOver
            animation.timingFunction = CAMediaTimingFunction(name: .linear)
        case .settling:
            animation.duration = settlesOver
            animation.timingFunction = CAMediaTimingFunction(name: .easeInEaseOut)
        }
        return animation
    }
}
