package org.resonate.android.ui.settings

import org.resonate.android.nativecore.DownloadAccount
import org.resonate.android.nativecore.MusicProvider

enum class ProviderField {
    SERVER,
    USER,
    PASSWORD,
    CLIENT_ID,
    CLIENT_SECRET,
    REFRESH_TOKEN,
}

val providersShown =
    listOf(MusicProvider.MONOCHROME, MusicProvider.HIFI_API, MusicProvider.SUBSONIC, MusicProvider.TIDAL)

fun providerName(provider: MusicProvider): String =
    when (provider) {
        MusicProvider.MONOCHROME -> "Monochrome"
        MusicProvider.HIFI_API -> "hifi-api"
        MusicProvider.SUBSONIC -> "Subsonic"
        MusicProvider.TIDAL -> "TIDAL"
    }

fun providerDetail(provider: MusicProvider): String =
    when (provider) {
        MusicProvider.MONOCHROME -> "A Monochrome server, or the hosted one"
        MusicProvider.HIFI_API -> "A hifi-api server, or the hosted one"
        MusicProvider.SUBSONIC -> "Your own Subsonic or Navidrome server"
        MusicProvider.TIDAL -> "Your TIDAL subscription"
    }

fun providerFields(provider: MusicProvider): List<ProviderField> =
    when (provider) {
        MusicProvider.MONOCHROME -> listOf(ProviderField.SERVER)
        MusicProvider.HIFI_API -> listOf(ProviderField.SERVER, ProviderField.PASSWORD)
        MusicProvider.SUBSONIC ->
            listOf(ProviderField.SERVER, ProviderField.USER, ProviderField.PASSWORD)
        MusicProvider.TIDAL ->
            listOf(ProviderField.CLIENT_ID, ProviderField.CLIENT_SECRET, ProviderField.REFRESH_TOKEN)
    }

fun accountIdentity(account: DownloadAccount): String = "${account.provider.name}:${account.refreshToken}"

fun DownloadAccount.valueOf(field: ProviderField): String =
    when (field) {
        ProviderField.SERVER -> server
        ProviderField.USER -> user
        ProviderField.PASSWORD -> password
        ProviderField.CLIENT_ID -> clientId
        ProviderField.CLIENT_SECRET -> clientSecret
        ProviderField.REFRESH_TOKEN -> refreshToken
    }

fun DownloadAccount.with(field: ProviderField, value: String): DownloadAccount =
    when (field) {
        ProviderField.SERVER -> copy(server = value)
        ProviderField.USER -> copy(user = value)
        ProviderField.PASSWORD -> copy(password = value)
        ProviderField.CLIENT_ID -> copy(clientId = value)
        ProviderField.CLIENT_SECRET -> copy(clientSecret = value)
        ProviderField.REFRESH_TOKEN -> copy(refreshToken = value)
    }

fun DownloadAccount.trimmed(): DownloadAccount =
    copy(server = server.trim(), user = user.trim(), clientId = clientId.trim())
