package org.resonate.android

import org.resonate.android.nativecore.DownloadAccount
import org.resonate.android.nativecore.MusicProvider

enum class Readiness {
    READY,
    MISSING_SERVER,
    MISSING_LOGIN,
    MISSING_CLIENT,
    NEEDS_SIGN_IN,
}

fun emptyAccount(provider: MusicProvider) = DownloadAccount(provider, "", "", "", "", "", "")

fun DownloadAccount.readiness(): Readiness =
    when (provider) {
        MusicProvider.MONOCHROME,
        MusicProvider.HIFI_API -> Readiness.READY
        MusicProvider.SUBSONIC ->
            when {
                server.isBlank() -> Readiness.MISSING_SERVER
                user.isBlank() || password.isEmpty() -> Readiness.MISSING_LOGIN
                else -> Readiness.READY
            }
        MusicProvider.TIDAL ->
            when {
                clientId.isBlank() -> Readiness.MISSING_CLIENT
                refreshToken.isBlank() -> Readiness.NEEDS_SIGN_IN
                else -> Readiness.READY
            }
    }

data class KeptAccount(val id: Long, val on: Boolean, val account: DownloadAccount) {
    val asked: Boolean
        get() = on && account.readiness() == Readiness.READY
}

data class ProviderAccountBook(val kept: List<KeptAccount> = emptyList()) {
    val asked: List<DownloadAccount>
        get() = kept.filter { it.asked }.map { it.account }

    val ready: Boolean
        get() = kept.any { it.asked }

    fun nextId(): Long = (kept.maxOfOrNull { it.id } ?: 0L) + 1

    fun account(id: Long): KeptAccount? = kept.firstOrNull { it.id == id }

    fun accepts(provider: MusicProvider): Boolean =
        provider != MusicProvider.TIDAL || kept.none { it.account.provider == MusicProvider.TIDAL }

    fun saved(id: Long, account: DownloadAccount): ProviderAccountBook =
        when {
            account(id) != null ->
                copy(kept = kept.map { if (it.id == id) it.copy(account = account) else it })
            accepts(account.provider) -> copy(kept = kept + KeptAccount(id, true, account))
            else -> this
        }

    fun turned(id: Long, on: Boolean): ProviderAccountBook =
        copy(kept = kept.map { if (it.id == id) it.copy(on = on) else it })

    fun removed(id: Long): ProviderAccountBook = copy(kept = kept.filter { it.id != id })

    fun renewed(token: String): ProviderAccountBook =
        copy(
            kept =
                kept.map {
                    if (it.account.provider == MusicProvider.TIDAL) {
                        it.copy(account = it.account.copy(refreshToken = token))
                    } else {
                        it
                    }
                }
        )

    companion object {
        const val UNSAVED = 0L
    }
}
