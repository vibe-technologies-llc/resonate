package org.resonate.android.ui.settings

import org.resonate.android.R
import org.resonate.android.Readiness
import org.resonate.android.Words
import org.resonate.android.nativecore.DownloadAccount
import org.resonate.android.nativecore.MusicProvider
import org.resonate.android.readiness

enum class ProviderField {
    SERVER,
    USER,
    PASSWORD,
    CLIENT_ID,
    CLIENT_SECRET,
    REFRESH_TOKEN,
}

val providersShown =
    listOf(
        MusicProvider.MONOCHROME,
        MusicProvider.HIFI_API,
        MusicProvider.SUBSONIC,
        MusicProvider.JELLYFIN,
        MusicProvider.TIDAL,
    )

fun providerName(provider: MusicProvider): String =
    when (provider) {
        MusicProvider.MONOCHROME -> "Monochrome"
        MusicProvider.HIFI_API -> "hifi-api"
        MusicProvider.SUBSONIC -> "Subsonic"
        MusicProvider.JELLYFIN -> "Jellyfin"
        MusicProvider.TIDAL -> "TIDAL"
    }

fun providerDetail(words: Words, provider: MusicProvider): String =
    words.string(
        when (provider) {
            MusicProvider.MONOCHROME -> R.string.provider_detail_monochrome
            MusicProvider.HIFI_API -> R.string.provider_detail_hifi_api
            MusicProvider.SUBSONIC -> R.string.provider_detail_subsonic
            MusicProvider.JELLYFIN -> R.string.provider_detail_jellyfin
            MusicProvider.TIDAL -> R.string.provider_detail_tidal
        }
    )

fun providerFields(provider: MusicProvider): List<ProviderField> =
    when (provider) {
        MusicProvider.MONOCHROME -> listOf(ProviderField.SERVER)
        MusicProvider.HIFI_API -> listOf(ProviderField.SERVER, ProviderField.PASSWORD)
        MusicProvider.SUBSONIC,
        MusicProvider.JELLYFIN ->
            listOf(ProviderField.SERVER, ProviderField.USER, ProviderField.PASSWORD)
        MusicProvider.TIDAL ->
            listOf(ProviderField.CLIENT_ID, ProviderField.CLIENT_SECRET, ProviderField.REFRESH_TOKEN)
    }

fun accountIdentity(account: DownloadAccount): String =
    "${account.provider.name}:${account.refreshToken}:${account.token}"

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

fun readinessWords(words: Words, readiness: Readiness): String =
    words.string(
        when (readiness) {
            Readiness.READY -> R.string.provider_ready
            Readiness.MISSING_SERVER -> R.string.provider_missing_server
            Readiness.MISSING_LOGIN -> R.string.provider_missing_login
            Readiness.MISSING_CLIENT -> R.string.provider_missing_client
            Readiness.NEEDS_SIGN_IN -> R.string.provider_needs_sign_in
        }
    )

fun accountLocation(words: Words, account: DownloadAccount): String {
    val server = account.server.trim().substringAfter("://").trimEnd('/')
    return when {
        server.isNotEmpty() -> server
        account.provider == MusicProvider.TIDAL ->
            words.string(R.string.provider_location_subscription)
        account.provider == MusicProvider.HIFI_API && account.password.isNotBlank() ->
            words.string(R.string.provider_location_own_server)
        account.provider == MusicProvider.SUBSONIC ||
            account.provider == MusicProvider.JELLYFIN ->
            words.string(R.string.provider_location_no_server)
        else -> words.string(R.string.provider_location_hosted)
    }
}

fun accountSummary(words: Words, account: DownloadAccount, on: Boolean): String {
    val status =
        if (on) readinessWords(words, account.readiness()) else words.string(R.string.provider_off)
    return "${accountLocation(words, account)} · $status"
}

fun accountsLead(words: Words, count: Int): String =
    if (count == 0) words.string(R.string.providers_lead_none)
    else words.plural(R.plurals.providers_lead, count, count)
