package org.resonate.android

import org.resonate.android.nativecore.DownloadAccount
import org.resonate.android.nativecore.MusicProvider

typealias Changes = Map<String, String?>

object ProviderAccountRecords {
    const val VERSION_KEY = "accounts-version"
    const val VERSION = "2"
    const val IDS_KEY = "accounts-ids"
    const val PREFIX = "account-"

    private const val PROVIDER = "provider"
    private const val SERVER = "server"
    private const val ON = "on"
    private val sealedFields =
        listOf("user", "password", "client-id", "client-secret", "refresh-token")
    private val legacyKeys = listOf(PROVIDER, SERVER) + sealedFields
    private const val FIRST_ID = 1L

    fun isCurrent(all: Map<String, *>): Boolean = all[VERSION_KEY] == VERSION

    fun migration(all: Map<String, *>, seal: (String) -> String): Changes {
        val legacyProvider = all[PROVIDER] as? String
        val removals = legacyKeys.associateWith { null }
        if (legacyProvider == null) {
            val hosted =
                ProviderAccountBook(
                    listOf(KeptAccount(FIRST_ID, true, emptyAccount(MusicProvider.MONOCHROME)))
                )
            return removals + encode(hosted, seal, emptySet())
        }
        val provider = MusicProvider.entries.find { it.name == legacyProvider }?.name
        val copied =
            buildMap<String, String?> {
                put(VERSION_KEY, VERSION)
                put(IDS_KEY, FIRST_ID.toString())
                put(key(FIRST_ID, PROVIDER), provider ?: MusicProvider.MONOCHROME.name)
                put(key(FIRST_ID, SERVER), all[SERVER] as? String ?: "")
                put(key(FIRST_ID, ON), true.toString())
                sealedFields.forEach { put(key(FIRST_ID, it), all[it] as? String ?: "") }
            }
        return removals + copied
    }

    fun decode(all: Map<String, *>, open: (String) -> String): ProviderAccountBook {
        val ids = (all[IDS_KEY] as? String).orEmpty().split(',').mapNotNull { it.toLongOrNull() }
        val kept =
            ids.mapNotNull { id ->
                val provider =
                    MusicProvider.entries.find { it.name == all[key(id, PROVIDER)] }
                        ?: return@mapNotNull null
                fun sealed(field: String) = open(all[key(id, field)] as? String ?: "")
                KeptAccount(
                    id = id,
                    on = all[key(id, ON)] != false.toString(),
                    account =
                        DownloadAccount(
                            provider = provider,
                            server = all[key(id, SERVER)] as? String ?: "",
                            user = sealed("user"),
                            password = sealed("password"),
                            clientId = sealed("client-id"),
                            clientSecret = sealed("client-secret"),
                            refreshToken = sealed("refresh-token"),
                        ),
                )
            }
        return ProviderAccountBook(kept)
    }

    fun encode(book: ProviderAccountBook, seal: (String) -> String, existing: Set<String>): Changes {
        val stale = existing.filter { it.startsWith(PREFIX) }.associateWith { null }
        val written =
            buildMap<String, String?> {
                put(VERSION_KEY, VERSION)
                put(IDS_KEY, book.kept.joinToString(",") { it.id.toString() })
                book.kept.forEach { kept ->
                    val account = kept.account
                    put(key(kept.id, PROVIDER), account.provider.name)
                    put(key(kept.id, SERVER), account.server)
                    put(key(kept.id, ON), kept.on.toString())
                    put(key(kept.id, "user"), seal(account.user))
                    put(key(kept.id, "password"), seal(account.password))
                    put(key(kept.id, "client-id"), seal(account.clientId))
                    put(key(kept.id, "client-secret"), seal(account.clientSecret))
                    put(key(kept.id, "refresh-token"), seal(account.refreshToken))
                }
            }
        return stale + written
    }

    private fun key(id: Long, field: String) = "$PREFIX$id-$field"
}
