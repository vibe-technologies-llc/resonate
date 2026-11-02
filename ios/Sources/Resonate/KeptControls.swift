import AVKit
import MediaPlayer
import UIKit

@MainActor
enum KeptControls {
    private static let barelyShown: CGFloat = 0.01

    static let route = KeptControl<AVRoutePickerView> {
        let picker = AVRoutePickerView()
        picker.prioritizesVideoDevices = false
        return picker
    }

    static let volume = KeptControl<MPVolumeView> {
        let driver = MPVolumeView(frame: .zero)
        driver.setValue(false, forKey: "showsRouteButton")
        driver.alpha = KeptControls.barelyShown
        driver.isUserInteractionEnabled = false
        return driver
    }

    static func warm() {
        route.control.layoutIfNeeded()
        volume.control.layoutIfNeeded()
    }
}

@MainActor
final class KeptControl<Control: UIView> {
    private let make: () -> Control
    private var made: Control?
    private let hosts = NSHashTable<KeptControlHost>.weakObjects()

    init(_ make: @escaping () -> Control) {
        self.make = make
    }

    var control: Control {
        if let made { return made }
        let fresh = make()
        made = fresh
        return fresh
    }

    func host() -> KeptControlHost {
        KeptControlHost(
            entered: { [unowned self] host in self.enter(host) },
            left: { [unowned self] host in self.leave(host) }
        )
    }

    private func enter(_ host: KeptControlHost) {
        hosts.add(host)
        host.adopt(control)
    }

    private func leave(_ host: KeptControlHost) {
        hosts.remove(host)
        guard let made, made.superview === host else { return }
        made.removeFromSuperview()
        hosts.allObjects.first { $0.window != nil }?.adopt(made)
    }
}

final class KeptControlHost: UIView {
    private let entered: (KeptControlHost) -> Void
    private let left: (KeptControlHost) -> Void

    init(entered: @escaping (KeptControlHost) -> Void, left: @escaping (KeptControlHost) -> Void) {
        self.entered = entered
        self.left = left
        super.init(frame: .zero)
        backgroundColor = .clear
        isOpaque = false
    }

    required init?(coder: NSCoder) {
        return nil
    }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        if window != nil {
            entered(self)
        } else {
            left(self)
        }
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        for held in subviews { held.frame = bounds }
    }

    func adopt(_ control: UIView) {
        guard control.superview !== self else { return }
        control.frame = bounds
        control.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        addSubview(control)
    }
}
