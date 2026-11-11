package org.resonate.android

import android.content.Context
import android.content.Intent
import android.content.SharedPreferences
import android.util.Log
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import org.resonate.android.nativecore.Exception as NativeFailure
import org.resonate.android.nativecore.InvitationKind
import org.resonate.android.nativecore.ListeningParty
import org.resonate.android.nativecore.MobilePlayer
import org.resonate.android.nativecore.SharedPlaylist
import org.resonate.android.nativecore.SyncConfig
import org.resonate.android.nativecore.invitationKind

private const val NAME_FOR_OTHERS_KEY = "shared-name"
private const val INVITATION_WAITS_FOR_SYNC_AT_MOST_MS = 20_000L

class Together(
    private val context: Context,
    private val library: LibraryRepository,
    private val settings: SharedPreferences,
    private val scope: CoroutineScope,
    private val online: StateFlow<Boolean>,
    private val syncConfig: StateFlow<SyncConfig?>,
    private val syncApplied: StateFlow<Boolean>,
    private val playlistsMoved: () -> Unit,
) {
    private val mutableParty = MutableStateFlow<ListeningParty?>(null)
    private val mutableLists = MutableStateFlow<List<SharedPlaylist>>(emptyList())
    private val mutableName = MutableStateFlow(settings.getString(NAME_FOR_OTHERS_KEY, "").orEmpty())

    val party = mutableParty.asStateFlow()
    val lists = mutableLists.asStateFlow()
    val nameForOthers = mutableName.asStateFlow()
    val arrived = MutableStateFlow<ArrivalPage?>(null)
    val playlistsAsked = MutableStateFlow(false)
    val partyOffered = MutableStateFlow<String?>(null)

    val keepsTheServiceForeground: Boolean
        get() = mutableParty.value?.let { !TogetherWords.isOver(it.connection) } == true

    fun begin(native: MobilePlayer) {
        native.togetherNamed(mutableName.value)
    }

    fun stopped() {
        mutableParty.value = null
        mutableLists.value = emptyList()
    }

    fun onlineTurned(enabled: Boolean) {
        if (!enabled && mutableParty.value != null) leaveParty()
    }

    fun nameForOthers(name: String) {
        val kept = name.trim()
        mutableName.value = kept
        settings.edit().putString(NAME_FOR_OTHERS_KEY, kept).apply()
        work { it.togetherNamed(kept) }
    }

    fun sharingNeedsSync(): String? {
        val config = syncConfig.value ?: return null
        return TogetherWords.sharingNeedsSync(
            library.words,
            config.enabled,
            config.nearbyOnly,
            online.value,
        )
    }

    fun arriving(link: String, party: Boolean) {
        if (party) partyOffered.value = link else arrive(link)
    }

    fun arrive(link: String) = work { native ->
        if (invitationKind(link) == null) {
            library.notice(R.string.together_not_an_invitation)
        } else {
            withTimeoutOrNull(INVITATION_WAITS_FOR_SYNC_AT_MOST_MS) { syncApplied.first { it } }
            accept(native, link)
        }
    }

    fun paste(clipboard: String?) {
        if (clipboard == null || invitationKind(clipboard) == null) {
            library.notice(R.string.together_nothing_to_paste)
        } else {
            arrive(clipboard)
        }
    }

    private fun accept(native: MobilePlayer, link: String) {
        val kind =
            try {
                native.acceptInvitation(link)
            } catch (off: NativeFailure.SyncOff) {
                library.notice(sharingNeedsSync() ?: off.message.orEmpty())
                return
            } catch (refused: NativeFailure.Sync) {
                Log.w("Resonate", "An invitation was not taken up", refused)
                library.notice(R.string.together_link_not_opened)
                return
            } ?: return library.notice(R.string.together_not_an_invitation)

        when (kind) {
            InvitationKind.PARTY -> {
                mutableParty.value = native.partyShown()
                holdTheService()
            }
            InvitationKind.PLAYLIST -> {
                mutableLists.value = native.sharedPlaylists()
                playlistsMoved()
            }
        }
        arrived.value = InvitationLinks.pageFor(kind)
    }

    fun beginParty() = work { native ->
        mutableParty.value = native.partyBegin()
        holdTheService()
    }

    fun leaveParty() = work { native ->
        native.partyLeave()
        mutableParty.value = null
    }

    suspend fun readParty() {
        mutableParty.value = withContext(Dispatchers.IO) { library.holding { it.partyShown() } }
    }

    fun turnOut(person: ByteArray) = work { native ->
        native.partyTurnOut(person)
        mutableParty.value = native.partyShown()
    }

    fun playNext(request: Int) = work { native ->
        if (!native.partyPlayNext(request.toUInt()))
            library.notice(R.string.together_not_found_to_play_next)
        mutableParty.value = native.partyShown()
    }

    fun passOver(request: Int) = work { native ->
        native.partyPassOver(request.toUInt())
        mutableParty.value = native.partyShown()
    }

    fun readLists() = work { native -> readLists(native) }

    fun readLists(native: MobilePlayer) {
        mutableLists.value = native.sharedPlaylists()
    }

    fun share(id: ULong) = work { native ->
        try {
            native.sharePlaylist(id)
            readLists(native)
            playlistsMoved()
        } catch (off: NativeFailure.SyncOff) {
            library.notice(sharingNeedsSync() ?: off.message.orEmpty())
        }
    }

    fun reset(id: ULong, removing: ByteArray?) = work { native ->
        native.sharedReset(id, removing)
        readLists(native)
    }

    fun leaveList(id: ULong) = work { native ->
        native.sharedLeave(id)
        readLists(native)
        playlistsMoved()
    }

    fun watch(id: ULong?) = work { it.sharedWatch(id) }

    private fun holdTheService() {
        try {
            context.startForegroundService(Intent(context, PartyService::class.java))
        } catch (failure: Exception) {
            Log.w("Resonate", "The party goes on without a foreground service", failure)
        }
    }

    private fun work(action: suspend (MobilePlayer) -> Unit) {
        scope.launch {
            try {
                library.holding { action(it) }
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failure: Exception) {
                library.report(failure)
            }
        }
    }
}
