package org.resonate.android

import java.text.Normalizer
import org.resonate.android.nativecore.StandingDocument

class StandingCopies(private val tree: DocumentTree) {
    fun standing(name: String, filed: List<List<String>>): List<StandingDocument> {
        val stem = name.substringBeforeLast('.')
        val atTheRoot =
            tree.children(tree.rootId).orEmpty().filter {
                !it.directory && composed(it.name).startsWith(composed(stem))
            }
        return atTheRoot.map { StandingDocument(tree.uri(it.id), it.name, emptyList()) } +
            filed.flatMap { folder ->
                val within = reached(folder) ?: return@flatMap emptyList()
                tree.children(within).orEmpty().filterNot { it.directory }.map {
                    StandingDocument(tree.uri(it.id), it.name, folder)
                }
            }
    }

    private fun reached(folder: List<String>): String? {
        if (!Ids.nameable(folder)) return null
        var at = tree.rootId
        for (part in folder) {
            val wanted = composed(part)
            at =
                tree.children(at).orEmpty().firstOrNull {
                    it.directory && composed(it.name) == wanted
                }?.id ?: return null
        }
        return at
    }

    companion object {
        fun composed(name: String): String = Normalizer.normalize(name, Normalizer.Form.NFC)

        fun relativePath(top: String, folder: List<String>): String =
            (listOf(top) + folder).joinToString("/", postfix = "/")
    }
}
