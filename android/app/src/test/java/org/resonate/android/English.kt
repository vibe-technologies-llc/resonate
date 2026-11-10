package org.resonate.android

import java.io.File
import java.util.Locale
import javax.xml.parsers.DocumentBuilderFactory
import org.w3c.dom.Element

object English : Words {
    private val values = File("src/main/res/values")

    private val strings: Map<String, String>
    private val plurals: Map<String, Map<String, String>>

    init {
        val foundStrings = mutableMapOf<String, String>()
        val foundPlurals = mutableMapOf<String, Map<String, String>>()
        val files = values.listFiles { file -> file.extension == "xml" }.orEmpty().sortedBy { it.name }
        for (file in files) {
            val root = DocumentBuilderFactory.newInstance().newDocumentBuilder().parse(file).documentElement
            for (element in root.children()) {
                val name = element.getAttribute("name")
                when (element.tagName) {
                    "string" -> foundStrings[name] = unescaped(element.textContent)
                    "plurals" ->
                        foundPlurals[name] =
                            element.children().associate { item ->
                                item.getAttribute("quantity") to unescaped(item.textContent)
                            }
                }
            }
        }
        strings = foundStrings
        plurals = foundPlurals
    }

    override fun string(id: Int, vararg args: Any): String {
        val text = strings.getValue(nameOf(R.string::class.java, id))
        return if (args.isEmpty()) text else String.format(Locale.ENGLISH, text, *args)
    }

    override fun plural(id: Int, count: Int, vararg args: Any): String {
        val forms = plurals.getValue(nameOf(R.plurals::class.java, id))
        val text = forms[if (count == 1) "one" else "other"] ?: forms.getValue("other")
        return if (args.isEmpty()) text else String.format(Locale.ENGLISH, text, *args)
    }

    private fun nameOf(kind: Class<*>, id: Int): String =
        kind.fields.first { it.getInt(null) == id }.name

    private fun Element.children(): List<Element> =
        (0 until childNodes.length).map { childNodes.item(it) }.filterIsInstance<Element>()

    private fun unescaped(raw: String): String {
        val text = StringBuilder()
        var quoted = false
        var at = 0
        while (at < raw.length) {
            val letter = raw[at]
            when {
                letter == '\\' && at + 1 < raw.length -> {
                    at++
                    when (val escaped = raw[at]) {
                        'n' -> text.append('\n')
                        't' -> text.append('\t')
                        'u' -> {
                            text.append(raw.substring(at + 1, at + 5).toInt(16).toChar())
                            at += 4
                        }
                        else -> text.append(escaped)
                    }
                }
                letter == '"' -> quoted = !quoted
                letter.isWhitespace() && !quoted -> {
                    if (text.isEmpty() || !text.last().isWhitespace()) text.append(' ')
                }
                else -> text.append(letter)
            }
            at++
        }
        return text.toString().trim()
    }
}
