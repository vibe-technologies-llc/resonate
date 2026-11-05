package org.resonate.android

import org.resonate.android.nativecore.InvitationKind

const val RESONATE_WEB_HOST = "resonate.tecnio.dev"

private const val RESONATE_SCHEME = "resonate"
private const val SECURE_WEB_SCHEME = "https"
private const val SHARED_LIST_HOST = "shared"
private const val PARTY_HOST = "party"
private const val SHARED_LIST_PAGE = "/s/"
private const val PARTY_PAGE = "/p/"

enum class OpenedLink {
    PAIRING,
    INVITATION,
    SOMETHING_ELSE,
}

enum class ArrivalPage {
    PARTY,
    PLAYLISTS,
}

object InvitationLinks {
    fun of(scheme: String?, host: String?, path: String?): OpenedLink =
        when (scheme?.lowercase()) {
            RESONATE_SCHEME ->
                when (host?.lowercase()) {
                    SHARED_LIST_HOST,
                    PARTY_HOST -> OpenedLink.INVITATION
                    else -> OpenedLink.PAIRING
                }
            SECURE_WEB_SCHEME ->
                if (host.equals(RESONATE_WEB_HOST, ignoreCase = true) && isAnInvitationPage(path))
                    OpenedLink.INVITATION
                else OpenedLink.SOMETHING_ELSE
            else -> OpenedLink.SOMETHING_ELSE
        }

    fun isAParty(scheme: String?, host: String?, path: String?): Boolean =
        when (scheme?.lowercase()) {
            RESONATE_SCHEME -> host.equals(PARTY_HOST, ignoreCase = true)
            SECURE_WEB_SCHEME ->
                host.equals(RESONATE_WEB_HOST, ignoreCase = true) &&
                    path?.startsWith(PARTY_PAGE) == true
            else -> false
        }

    fun pageFor(kind: InvitationKind): ArrivalPage =
        when (kind) {
            InvitationKind.PARTY -> ArrivalPage.PARTY
            InvitationKind.PLAYLIST -> ArrivalPage.PLAYLISTS
        }

    private fun isAnInvitationPage(path: String?): Boolean =
        path != null && (path.startsWith(SHARED_LIST_PAGE) || path.startsWith(PARTY_PAGE))
}
