import Foundation
import Network
import ResonateNative

final class NearbyDevices: @unchecked Sendable {
    private static let service = "_resonate-sync._udp"
    private static let recordKey = "b"
    private static let toldAgainAfter: TimeInterval = 5

    private let native: MobilePlayer
    private let queue = DispatchQueue(label: "org.resonate.nearby")
    private let instance = UUID().uuidString.replacingOccurrences(of: "-", with: "").lowercased()
    private var listener: NWListener?
    private var advertised: String?
    private var browser: NWBrowser?
    private var seen: Set<String> = []
    private var toldAt = Date.distantPast

    init(native: MobilePlayer) {
        self.native = native
    }

    func follow(record: String?) {
        queue.async { self.followed(record) }
    }

    func refresh() {
        queue.async { self.toldAt = .distantPast }
    }

    func restart() {
        queue.async {
            self.stopped()
            self.toldAt = .distantPast
        }
    }

    private func followed(_ record: String?) {
        guard let record else {
            stopped()
            return
        }
        if browser == nil { browse() }
        if record != advertised { advertise(record) }
        if Date().timeIntervalSince(toldAt) >= Self.toldAgainAfter {
            toldAt = Date()
            for heard in seen { native.syncNearbyHeard(record: heard) }
        }
    }

    private func advertise(_ record: String) {
        listener?.cancel()
        listener = nil
        advertised = nil
        do {
            let listening = try NWListener(using: .udp)
            listening.service = NWListener.Service(
                name: instance,
                type: Self.service,
                domain: nil,
                txtRecord: NWTXTRecord([Self.recordKey: record])
            )
            listening.newConnectionHandler = { connection in connection.cancel() }
            listening.stateUpdateHandler = { [weak self] state in
                if case .failed = state { self?.advertised = nil }
            }
            listening.start(queue: queue)
            listener = listening
            advertised = record
        } catch {
            advertised = nil
        }
    }

    private func browse() {
        let browsing = NWBrowser(
            for: .bonjourWithTXTRecord(type: Self.service, domain: nil),
            using: .udp
        )
        browsing.browseResultsChangedHandler = { [weak self] results, _ in
            self?.found(results)
        }
        browsing.stateUpdateHandler = { [weak self] state in
            if case .failed = state {
                self?.browser?.cancel()
                self?.browser = nil
            }
        }
        browsing.start(queue: queue)
        browser = browsing
    }

    private func found(_ results: Set<NWBrowser.Result>) {
        var records: Set<String> = []
        for result in results {
            if case let .bonjour(text) = result.metadata, let record = text[Self.recordKey] {
                records.insert(record)
            }
        }
        for record in records.subtracting(seen) {
            native.syncNearbyHeard(record: record)
        }
        seen = records
    }

    private func stopped() {
        listener?.cancel()
        listener = nil
        advertised = nil
        browser?.cancel()
        browser = nil
        seen = []
    }
}
