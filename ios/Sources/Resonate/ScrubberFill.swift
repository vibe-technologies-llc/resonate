import QuartzCore
import ResonateLogic
import SwiftUI
import UIKit

enum ScrubberPace: Equatable {
    case snap
    case flow(over: Double)
    case settle(over: Double)
}

struct ScrubberCourse: Equatable {
    let to: Double
    let pace: ScrubberPace
}

struct ScrubberFill: UIViewRepresentable {
    let fraction: Double
    let course: ScrubberCourse
    let ink: Double
    let dark: Bool
    let turnsInk: Bool

    func makeUIView(context: Context) -> ScrubberFillView {
        ScrubberFillView(frame: .zero)
    }

    func updateUIView(_ view: ScrubberFillView, context: Context) {
        let traits = UITraitCollection(userInterfaceStyle: dark ? .dark : .light)
        view.ink(UIColor.label.withAlphaComponent(ink).resolvedColor(with: traits).cgColor, turning: turnsInk)
        view.move(to: fraction, along: course)
    }
}

final class ScrubberFillView: UIView {
    private static let flowKey = "flow"
    private static let inkKey = "ink"
    private static let inkTurnsOver: CFTimeInterval = 0.3
    private static let narrowest: CGFloat = 0.0001
    private static let unmoved: CGFloat = 0.00001
    private static let unanimated: [String: any CAAction] = [
        "bounds": NSNull(),
        "position": NSNull(),
        "transform": NSNull(),
        "backgroundColor": NSNull(),
    ]
    private static let settling = CAMediaTimingFunction(
        controlPoints: Float(MotionCurve.settling.x1),
        Float(MotionCurve.settling.y1),
        Float(MotionCurve.settling.x2),
        Float(MotionCurve.settling.y2)
    )

    private let fill = CALayer()
    private var followed: ScrubberCourse?
    private var shown: CGFloat?
    private var inked: CGColor?

    override init(frame: CGRect) {
        super.init(frame: frame)
        isUserInteractionEnabled = false
        isOpaque = false
        backgroundColor = .clear
        isAccessibilityElement = false
        layer.masksToBounds = true
        layer.cornerCurve = .circular
        fill.anchorPoint = CGPoint(x: 0, y: 0.5)
        fill.allowsEdgeAntialiasing = true
        fill.actions = Self.unanimated
        fill.transform = CATransform3DMakeScale(Self.narrowest, 1, 1)
        layer.addSublayer(fill)
    }

    required init?(coder: NSCoder) {
        return nil
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        layer.cornerRadius = bounds.height / 2
        fill.bounds = CGRect(origin: .zero, size: bounds.size)
        fill.position = CGPoint(x: 0, y: bounds.midY)
    }

    func move(to fraction: Double, along course: ScrubberCourse) {
        let target = min(max(CGFloat(fraction), Self.narrowest), 1)
        let newCourse = course != followed
        followed = course
        guard target != shown || newCourse else { return }

        let from = fill.presentation()?.transform.m11 ?? fill.transform.m11
        let moving = shown != nil && newCourse ? course.pace : .snap
        shown = target
        fill.removeAnimation(forKey: Self.flowKey)
        fill.transform = CATransform3DMakeScale(target, 1, 1)
        guard let flow = Self.timed(moving), abs(from - target) > Self.unmoved else { return }

        flow.fromValue = from
        flow.toValue = target
        fill.add(flow, forKey: Self.flowKey)
    }

    func ink(_ colour: CGColor, turning: Bool) {
        guard inked.map({ $0 != colour }) ?? true else { return }
        let before: CGColor? = inked == nil ? nil : fill.presentation()?.backgroundColor ?? fill.backgroundColor
        inked = colour
        fill.removeAnimation(forKey: Self.inkKey)
        fill.backgroundColor = colour
        guard turning, let before else { return }

        let turn = CABasicAnimation(keyPath: "backgroundColor")
        turn.fromValue = before
        turn.toValue = colour
        turn.duration = Self.inkTurnsOver
        turn.timingFunction = Self.settling
        fill.add(turn, forKey: Self.inkKey)
    }

    private static func timed(_ pace: ScrubberPace) -> CABasicAnimation? {
        let animation = CABasicAnimation(keyPath: "transform.scale.x")
        switch pace {
        case .snap:
            return nil
        case .flow(let seconds):
            animation.duration = seconds
            animation.timingFunction = CAMediaTimingFunction(name: .linear)
        case .settle(let seconds):
            animation.duration = seconds
            animation.timingFunction = settling
        }
        return animation
    }
}
