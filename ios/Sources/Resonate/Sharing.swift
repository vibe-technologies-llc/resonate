import SwiftUI
import UIKit

struct SharedLink: Identifiable {
    let id = UUID()
    let title: String
    let url: URL
}

struct ActivitySheet: UIViewControllerRepresentable {
    let link: URL
    let title: String

    func makeUIViewController(context: Context) -> UIActivityViewController {
        let controller = UIActivityViewController(activityItems: [link], applicationActivities: nil)
        controller.title = title
        return controller
    }

    func updateUIViewController(_ controller: UIActivityViewController, context: Context) {}
}
