package org.resonate.android

import org.resonate.android.nativecore.FiledSong
import org.resonate.android.nativecore.MovedDocument

data class TreeEntry(val id: String, val name: String, val directory: Boolean)

interface DocumentTree {
    val rootId: String

    fun documentId(uri: String): String?

    fun uri(documentId: String): String

    fun children(directoryId: String): List<TreeEntry>?

    fun makeDirectory(parentId: String, name: String): String?

    fun move(documentId: String, fromId: String, toId: String): String?

    fun rename(documentId: String, name: String): String?

    fun delete(documentId: String): Boolean
}

data class Filed(
    val moved: List<MovedDocument>,
    val back: List<FiledSong>,
    val failed: Int,
)

class FolderFiling(private val tree: DocumentTree) {
    private val listings = HashMap<String, MutableMap<String, TreeEntry>>()
    private val directories = HashMap<String, String>()
    private val left = LinkedHashSet<String>()
    private val created = LinkedHashSet<String>()

    fun file(songs: List<FiledSong>): Filed {
        val moved = ArrayList<MovedDocument>()
        val back = ArrayList<FiledSong>()
        var failed = 0
        for (song in songs) {
            val placed = placeOrNull(song)
            if (placed == null) {
                failed++
                continue
            }
            moved.add(MovedDocument(song.id, tree.uri(placed.id)))
            back.add(FiledSong(song.id, tree.uri(placed.id), placed.before))
            left.add(placed.sourceDirectory)
        }
        prune()
        return Filed(moved, back, failed)
    }

    private class Placed(val id: String, val before: List<String>, val sourceDirectory: String)

    private class Unlistable : Exception()

    private fun placeOrNull(song: FiledSong): Placed? =
        try {
            place(song)
        } catch (unreadable: Unlistable) {
            null
        }

    private fun place(song: FiledSong): Placed? {
        val sourceId = tree.documentId(song.from) ?: return null
        val before = Ids.relative(sourceId, tree.rootId) ?: return null
        if (!Ids.nameable(song.to)) return null

        val sourceName = before.last()
        val sourceDirectory = Ids.parent(sourceId)
        val source =
            listing(sourceDirectory)[sourceName]?.takeIf { it.id == sourceId && !it.directory }
                ?: return null

        val name = song.to.last()
        val folders = song.to.dropLast(1)
        val existing = directoryAt(folders, create = false)
        if (existing != null && taken(existing, name, sourceId)) return null

        val destination = existing ?: directoryAt(folders, create = true) ?: return null
        val movedId = transfer(source, sourceDirectory, destination, name) ?: return null

        sidecarOrNot(sourceDirectory, sourceName, destination, name)
        return Placed(movedId, before, sourceDirectory)
    }

    private fun sidecarOrNot(
        sourceDirectory: String,
        sourceName: String,
        destination: String,
        name: String,
    ) {
        try {
            sidecarAlong(sourceDirectory, sourceName, destination, name)
        } catch (unreadable: Unlistable) {
            return
        }
    }

    private fun sidecarAlong(
        sourceDirectory: String,
        sourceName: String,
        destination: String,
        name: String,
    ) {
        val sidecar =
            listing(sourceDirectory).values.firstOrNull {
                !it.directory && it.name.equals(Ids.sidecarOf(sourceName), ignoreCase = true)
            } ?: return
        val sidecarName = Ids.sidecarOf(name)
        if (taken(destination, sidecarName, sidecar.id)) return
        transfer(sidecar, sourceDirectory, destination, sidecarName)
    }

    private fun transfer(
        entry: TreeEntry,
        fromDirectory: String,
        toDirectory: String,
        name: String,
    ): String? {
        if (fromDirectory == toDirectory) {
            return if (entry.name == name) entry.id else renamed(entry, fromDirectory, name)
        }
        return when {
            !taken(toDirectory, entry.name, entry.id) -> movedThenRenamed(entry, fromDirectory, toDirectory, name)
            !taken(fromDirectory, name, entry.id) -> renamedThenMoved(entry, fromDirectory, toDirectory, name)
            else -> null
        }
    }

    private fun movedThenRenamed(
        entry: TreeEntry,
        fromDirectory: String,
        toDirectory: String,
        name: String,
    ): String? {
        val movedId = tree.move(entry.id, fromDirectory, toDirectory) ?: return null
        listing(fromDirectory).remove(entry.name)
        val standing = TreeEntry(movedId, entry.name, false)
        listing(toDirectory)[entry.name] = standing
        if (entry.name == name) return movedId

        val renamedId = renamed(standing, toDirectory, name)
        if (renamedId != null) return renamedId

        val returned = tree.move(movedId, toDirectory, fromDirectory)
        if (returned != null) {
            listing(toDirectory).remove(entry.name)
            listing(fromDirectory)[entry.name] = TreeEntry(returned, entry.name, false)
        }
        return null
    }

    private fun renamedThenMoved(
        entry: TreeEntry,
        fromDirectory: String,
        toDirectory: String,
        name: String,
    ): String? {
        val renamedId = renamed(entry, fromDirectory, name) ?: return null
        val standing = TreeEntry(renamedId, name, false)
        val movedId = tree.move(renamedId, fromDirectory, toDirectory)
        if (movedId != null) {
            listing(fromDirectory).remove(name)
            listing(toDirectory)[name] = TreeEntry(movedId, name, false)
            return movedId
        }

        val restored = tree.rename(renamedId, entry.name)
        if (restored != null) {
            listing(fromDirectory).remove(name)
            listing(fromDirectory)[entry.name] = TreeEntry(restored, entry.name, false)
        } else {
            listing(fromDirectory)[name] = standing
        }
        return null
    }

    private fun renamed(entry: TreeEntry, directory: String, name: String): String? {
        val renamedId = tree.rename(entry.id, name) ?: return null
        listing(directory).remove(entry.name)
        listing(directory)[name] = TreeEntry(renamedId, name, false)
        return renamedId
    }

    private fun taken(directory: String, name: String, except: String): Boolean =
        listing(directory).values.any { it.name.equals(name, ignoreCase = true) && it.id != except }

    private fun listing(directory: String): MutableMap<String, TreeEntry> =
        listings.getOrPut(directory) {
            (tree.children(directory) ?: throw Unlistable()).associateByTo(LinkedHashMap()) {
                it.name
            }
        }

    private fun directoryAt(folders: List<String>, create: Boolean): String? {
        var current = tree.rootId
        var path = ""
        for (folder in folders) {
            path = if (path.isEmpty()) folder else "$path/$folder"
            val known = directories[path]
            if (known != null) {
                current = known
                continue
            }
            val entry = named(listing(current).values, folder)
            val next =
                when {
                    entry != null && entry.directory -> entry.id
                    entry != null -> return null
                    !create -> return null
                    else -> made(current, folder) ?: return null
                }
            directories[path] = next
            current = next
        }
        return current
    }

    private fun named(entries: Collection<TreeEntry>, name: String): TreeEntry? =
        entries.firstOrNull { it.name == name }
            ?: entries.firstOrNull { it.name.equals(name, ignoreCase = true) }

    private fun made(parent: String, name: String): String? {
        val id = tree.makeDirectory(parent, name) ?: return null
        created.add(id)
        listing(parent)[name] = TreeEntry(id, name, true)
        return id
    }

    private fun prune() {
        val visited = HashSet<String>()
        for (start in (left + created).sortedByDescending { Ids.depth(it) }) {
            var directory = start
            while (Ids.below(directory, tree.rootId) && visited.add(directory)) {
                val held = tree.children(directory)
                if (held == null || held.isNotEmpty() || !tree.delete(directory)) break
                directory = Ids.parent(directory)
            }
        }
    }
}

object Ids {
    const val SIDECAR_EXTENSION = ".lrc"

    private val UNNAMEABLE = setOf("", ".", "..")

    fun relative(documentId: String, rootId: String): List<String>? {
        val prefix = if (rootId.endsWith(":")) rootId else "$rootId/"
        if (!documentId.startsWith(prefix) || documentId.length == prefix.length) return null
        return documentId.removePrefix(prefix).split('/')
    }

    fun below(documentId: String, rootId: String): Boolean = relative(documentId, rootId) != null

    fun parent(documentId: String): String = documentId.substringBeforeLast('/')

    fun depth(documentId: String): Int = documentId.count { it == '/' }

    fun nameable(components: List<String>): Boolean =
        components.isNotEmpty() &&
            components.none { it in UNNAMEABLE || it.contains('/') || it.contains('\u0000') }

    fun sidecarOf(name: String): String = name.substringBeforeLast('.', name) + SIDECAR_EXTENSION
}
