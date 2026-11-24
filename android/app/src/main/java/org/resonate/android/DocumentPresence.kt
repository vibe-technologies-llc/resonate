package org.resonate.android

import java.io.FileNotFoundException

enum class OpenFailure {
    NotFound,
    Refused,
}

object DocumentPresence {
    fun failure(thrown: Exception, presence: () -> RowPresence): OpenFailure =
        when (thrown) {
            is FileNotFoundException,
            is IllegalArgumentException ->
                if (asked(presence) == RowPresence.Absent) OpenFailure.NotFound
                else OpenFailure.Refused
            else -> OpenFailure.Refused
        }

    fun inTree(tree: DocumentTree, documentId: String): RowPresence {
        val steps = Ids.relative(documentId, tree.rootId) ?: return RowPresence.Unknown
        val prefix = if (tree.rootId.endsWith(":")) tree.rootId else "${tree.rootId}/"
        val chain = steps.indices.map { depth ->
            prefix + steps.subList(0, depth + 1).joinToString("/")
        }
        return presenceAt(tree, chain, chain.lastIndex)
    }

    private fun presenceAt(tree: DocumentTree, chain: List<String>, depth: Int): RowPresence {
        val folder = if (depth == 0) tree.rootId else chain[depth - 1]
        val listed = tree.children(folder)
        return when {
            listed != null ->
                if (listed.any { it.id == chain[depth] }) RowPresence.Standing
                else RowPresence.Absent
            depth == 0 -> RowPresence.Unknown
            presenceAt(tree, chain, depth - 1) == RowPresence.Absent -> RowPresence.Absent
            else -> RowPresence.Unknown
        }
    }

    private fun asked(presence: () -> RowPresence): RowPresence =
        try {
            presence()
        } catch (failure: Exception) {
            RowPresence.Unknown
        }
}
