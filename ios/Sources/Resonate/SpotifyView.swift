import AuthenticationServices
import ResonateLogic
import ResonateNative
import SwiftUI

struct SpotifyView: View {
    @Environment(Library.self) private var library
    @Environment(\.webAuthenticationSession) private var webAuthenticationSession
    @State private var clientId = ""

    var body: some View {
        let features = library.features
        let account = features.spotifyAccount
        ThemedForm {
            Section {
                TextField("Client ID", text: $clientId)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                if clientId.trimmingCharacters(in: .whitespaces) != account.clientId {
                    Button("Save") { features.keepSpotifyClient(clientId) }
                }
            } header: {
                Text("Your app")
            } footer: {
                Text(SpotifyWords.register(redirect: spotifyRedirect()))
            }
            Section {
                if account.signedIn {
                    Button(features.followingProfile ? "Reading the account…" : "Follow the account") {
                        features.followProfile(spotifyAccountLink(), keptInStep: true)
                    }
                    .disabled(!features.online || features.followingProfile)
                    Button("Move to Resonate once") {
                        features.followProfile(spotifyAccountLink(), keptInStep: false)
                    }
                    .disabled(!features.online || features.followingProfile)
                    Button("Sign out of Spotify", role: .destructive) { features.signOutOfSpotify() }
                } else if features.spotifySigningIn {
                    Text("Approve the sign-in on the page that opened")
                    Button("Cancel sign-in") { features.cancelSpotifySignIn() }
                } else {
                    Button("Sign in to Spotify") { features.signInToSpotify(clientId) }
                        .disabled(!features.online || !SpotifyWords.canSignIn(clientId: clientId))
                }
            } header: {
                Text("Your account")
            } footer: {
                Text(account.signedIn ? SpotifyWords.whatFollowingDoes : SpotifyWords.whatSpotifyAllows)
            }
        }
        .navigationTitle("Spotify")
        .onAppear { clientId = account.clientId }
        .onChange(of: features.spotifyAccount) { _, kept in clientId = kept.clientId }
        .task(id: features.spotifyAddress) {
            guard let address = features.spotifyAddress, let page = URL(string: address) else { return }
            _ = try? await webAuthenticationSession.authenticate(
                using: page,
                callback: .customScheme(SpotifyWords.neverCalledBack),
                additionalHeaderFields: [:]
            )
            if !Task.isCancelled { features.cancelSpotifySignIn() }
        }
    }
}
