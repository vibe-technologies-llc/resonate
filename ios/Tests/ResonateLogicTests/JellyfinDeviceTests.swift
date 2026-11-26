import Foundation
import ResonateNative
import Testing

@testable import ResonateLogic

@Suite struct JellyfinDeviceTests {
    private func jellyfin(
        server: String = "https://a",
        user: String = "me",
        token: String = "",
        userId: String = "",
        device: String = ""
    ) -> DownloadAccount {
        DownloadAccount(
            provider: .jellyfin, server: server, user: user, password: "pw", clientId: "", clientSecret: "",
            refreshToken: "", token: token, userId: userId, device: device
        )
    }

    private func minted(_ device: String, server: String = "https://a", user: String = "me") -> MintedJellyfinDevice {
        MintedJellyfinDevice(server: server, user: user, device: device)
    }

    @Test func aMintedDeviceIsKeptOnTheJellyfinAccountThatSignedInUnderIt() {
        var accounts = ProviderAccounts(kept: [jellyfin()])

        let first = accounts.keep(minted: [minted("install-1")])

        #expect(first)
        #expect(accounts.account(.jellyfin).device == "install-1")
        #expect(accounts.asked.filter { $0.provider == .jellyfin }.map(\.device) == ["install-1"])

        let second = accounts.keep(minted: [minted("install-2")])

        #expect(!second)
        #expect(accounts.account(.jellyfin).device == "install-1")
    }

    @Test func aDeviceMintedForAnotherServerOrUserIsLeftAlone() {
        var accounts = ProviderAccounts(kept: [jellyfin()])

        let elsewhere = accounts.keep(minted: [minted("other", server: "https://b")])
        let another = accounts.keep(minted: [minted("other", user: "you")])

        #expect(!elsewhere)
        #expect(!another)
        #expect(accounts.account(.jellyfin).device.isEmpty)
    }

    @Test func aSecondSignInIsSentUnderTheDeviceTheFirstWasKept() {
        var accounts = ProviderAccounts(kept: [jellyfin()])
        accounts.keep(minted: [minted("install-1")])

        accounts.keep(jellyfin())

        #expect(accounts.account(.jellyfin).device == "install-1")
        #expect(!accounts.account(.jellyfin).holdsAQuickConnectSession)
        #expect(accounts.asked.filter { $0.provider == .jellyfin }.map(\.device) == ["install-1"])
    }

    @Test func aSessionKeepsTheDeviceItWasIssuedToAndLettingItGoTakesItAway() {
        var accounts = ProviderAccounts(kept: [jellyfin(token: "t", userId: "u", device: "issued")])

        let whileHeld = accounts.keep(minted: [minted("install-1")])

        #expect(!whileHeld)
        #expect(accounts.account(.jellyfin).device == "issued")

        var letGo = accounts.account(.jellyfin)
        letGo.letGoOfTheQuickConnectSession()
        accounts.keep(letGo)

        #expect(accounts.account(.jellyfin).device.isEmpty)

        let afterwards = accounts.keep(minted: [minted("install-1")])

        #expect(accounts.account(.jellyfin).device == "install-1")
        #expect(afterwards)
    }

    @Test func typingAPasswordKeepsTheInstallDeviceButLeavesASessionsAlone() {
        var installed = jellyfin(device: "install-1")
        installed.letGoOfTheQuickConnectSession()

        var session = jellyfin(token: "t", userId: "u", device: "issued")
        session.letGoOfTheQuickConnectSession()

        #expect(installed.device == "install-1")
        #expect(session.device.isEmpty)
        #expect(session.token.isEmpty)
    }
}
