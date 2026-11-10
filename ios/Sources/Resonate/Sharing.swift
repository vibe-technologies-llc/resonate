import ResonateNative
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

struct SharingSheet: ViewModifier {
    @Environment(Library.self) private var library
    @Environment(Navigation.self) private var navigation
    let insidePlayer: Bool

    func body(content: Content) -> some View {
        content.sheet(item: Binding(
            get: { navigation.nowPlaying == insidePlayer ? library.features.sharing : nil },
            set: { library.features.sharing = $0 }
        )) { shared in
            ActivitySheet(link: shared.url, title: shared.title)
                .sheetSized()
                .presentationDetents([.medium, .large])
        }
    }
}

struct ShareSongButton: View {
    @Environment(Library.self) private var library
    let track: Track

    var body: some View {
        Button("sharing.share-link", systemImage: "square.and.arrow.up") { library.features.share(track) }
    }
}
