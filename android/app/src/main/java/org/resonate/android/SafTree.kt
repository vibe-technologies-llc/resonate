package org.resonate.android

import android.content.Context
import android.net.Uri
import android.provider.DocumentsContract
import android.util.Log

class SafTree(private val context: Context, private val tree: Uri) : DocumentTree {
    private val resolver
        get() = context.contentResolver

    override val rootId: String = DocumentsContract.getTreeDocumentId(tree)

    override fun documentId(uri: String): String? =
        try {
            val parsed = Uri.parse(uri)
            parsed
                .takeIf {
                    it.authority == tree.authority &&
                        DocumentsContract.getTreeDocumentId(it) == rootId
                }
                ?.let(DocumentsContract::getDocumentId)
        } catch (foreign: IllegalArgumentException) {
            null
        }

    override fun uri(documentId: String): String = documentUri(documentId).toString()

    override fun children(directoryId: String): List<TreeEntry>? {
        val columns =
            arrayOf(
                DocumentsContract.Document.COLUMN_DOCUMENT_ID,
                DocumentsContract.Document.COLUMN_DISPLAY_NAME,
                DocumentsContract.Document.COLUMN_MIME_TYPE,
            )
        val listed = DocumentsContract.buildChildDocumentsUriUsingTree(tree, directoryId)
        return refusable("A folder could not be listed") {
            resolver.query(listed, columns, null, null, null)?.use { cursor ->
                buildList {
                    while (cursor.moveToNext()) {
                        val name = cursor.getString(1) ?: continue
                        add(
                            TreeEntry(
                                cursor.getString(0),
                                name,
                                cursor.getString(2) == DocumentsContract.Document.MIME_TYPE_DIR,
                            )
                        )
                    }
                }
            }
        }
    }

    override fun makeDirectory(parentId: String, name: String): String? =
        refusable("A folder could not be made") {
            DocumentsContract.createDocument(
                    resolver,
                    documentUri(parentId),
                    DocumentsContract.Document.MIME_TYPE_DIR,
                    name,
                )
                ?.let(DocumentsContract::getDocumentId)
        }

    override fun move(documentId: String, fromId: String, toId: String): String? =
        refusable("A document could not be moved") {
            DocumentsContract.moveDocument(
                    resolver,
                    documentUri(documentId),
                    documentUri(fromId),
                    documentUri(toId),
                )
                ?.let(DocumentsContract::getDocumentId)
        }

    override fun rename(documentId: String, name: String): String? =
        refusable("A document could not be renamed") {
            DocumentsContract.renameDocument(resolver, documentUri(documentId), name)
                ?.let(DocumentsContract::getDocumentId)
        }

    override fun delete(documentId: String): Boolean =
        refusable("A folder could not be removed") {
            DocumentsContract.deleteDocument(resolver, documentUri(documentId))
        } ?: false

    private fun documentUri(documentId: String): Uri =
        DocumentsContract.buildDocumentUriUsingTree(tree, documentId)

    private fun <T> refusable(said: String, action: () -> T?): T? =
        try {
            action()
        } catch (refused: Exception) {
            Log.w("Resonate", said, refused)
            null
        }
}
