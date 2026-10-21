package org.resonate.android.ui.search

enum class OnlineKind(val label: String) {
    SONGS("Songs"),
    ALBUMS("Albums"),
    ARTISTS("Artists"),
}

sealed interface OnlineView {
    data class Found(val kind: OnlineKind) : OnlineView

    data class Discography(val artist: String, val name: String) : OnlineView

    data class Editions(val group: String, val title: String, val artist: String) : OnlineView

    data class Release(val id: String, val title: String, val detail: String) : OnlineView

    data class Link(val text: String) : OnlineView
}

sealed interface OnlineRequest {
    data class Songs(val text: String) : OnlineRequest

    data class Albums(val text: String) : OnlineRequest

    data class Artists(val text: String) : OnlineRequest

    data class Discography(val artist: String) : OnlineRequest

    data class Editions(val group: String) : OnlineRequest

    data class Release(val id: String) : OnlineRequest

    data class Link(val text: String) : OnlineRequest
}

fun shows(view: OnlineView, query: String): OnlineRequest? {
    val text = query.trim()
    return when (view) {
        is OnlineView.Found ->
            if (text.length < ONLINE_TEXT_AT_LEAST || isLink(text)) null
            else
                when (view.kind) {
                    OnlineKind.SONGS -> OnlineRequest.Songs(text)
                    OnlineKind.ALBUMS -> OnlineRequest.Albums(text)
                    OnlineKind.ARTISTS -> OnlineRequest.Artists(text)
                }
        is OnlineView.Discography -> OnlineRequest.Discography(view.artist)
        is OnlineView.Editions -> OnlineRequest.Editions(view.group)
        is OnlineView.Release -> OnlineRequest.Release(view.id)
        is OnlineView.Link -> OnlineRequest.Link(view.text)
    }
}

data class OnlineTrail(
    val kind: OnlineKind = OnlineKind.SONGS,
    val views: List<OnlineView> = listOf(OnlineView.Found(kind)),
) {
    init {
        require(views.isNotEmpty())
    }

    val top: OnlineView
        get() = views.last()

    val nested: Boolean
        get() = views.size > 1

    fun opened(view: OnlineView): OnlineTrail =
        if (top == view) this else copy(views = views + view)

    fun back(): OnlineTrail = if (nested) copy(views = views.dropLast(1)) else this

    fun rooted(query: String, kind: OnlineKind = this.kind): OnlineTrail =
        OnlineTrail(
            kind,
            listOf(if (isLink(query)) OnlineView.Link(query.trim()) else OnlineView.Found(kind)),
        )
}
