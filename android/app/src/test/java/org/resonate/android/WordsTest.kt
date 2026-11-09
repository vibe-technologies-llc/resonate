package org.resonate.android

import java.io.File
import org.junit.Assert.assertTrue
import org.junit.Test

class WordsTest {
    private val sources = File("src/main/java")

    @Test
    fun everyWordTheAppShowsIsSaidThroughItsResources() {
        val found =
            sources
                .walkTopDown()
                .filter { it.isFile && it.extension == "kt" }
                .associate { file ->
                    file.relativeTo(sources).invariantSeparatorsPath to
                        Prose.of(file.readText()).filterNot { it.text in PROPER }
                }
                .filterValues { it.isNotEmpty() }

        val unlisted = found.filterKeys { it !in STILL_TO_MOVE }
        val moved = STILL_TO_MOVE.filter { it !in found }

        assertTrue(
            unlisted.entries.joinToString("\n") { (file, prose) ->
                prose.joinToString("\n") { "$file:${it.line}: \"${it.text}\"" }
            },
            unlisted.isEmpty(),
        )
        assertTrue("Moved already, so no longer still to move: $moved", moved.isEmpty())
    }

    @Test
    fun proseIsTextAListenerReadsAndNotAKeyOrALogLine() {
        val source =
            """
            val key = "download-wants-automatically"
            Log.w("Resonate", "A library task failed", failure)
            require(count > 0) { "A count must be above zero" }
            Text("Play next")
            val shown = "${'$'}{count} songs"
            """
                .trimIndent()

        val prose = Prose.of(source).map { it.text }

        assertTrue(prose.toString(), prose == listOf("Play next", "{} songs"))
    }

    private companion object {
        val PROPER =
            setOf(
                "Resonate",
                "ResonateNative",
                "Monochrome",
                "Subsonic",
                "Jellyfin",
                "TIDAL",
                "Spotify",
                "MusicBrainz",
                "ListenBrainz",
                "Last.fm",
                "Navidrome",
                "AutoEq",
                "EqualizerAPO",
                "GraphicEQ",
                "ReplayGain",
                "Bluetooth",
                "AndroidKeyStore",
                "AES/GCM/NoPadding",
            )

        val STILL_TO_MOVE = setOf<String>()
    }
}

data class Prose(val line: Int, val text: String) {
    companion object {
        private val SAID_FOR_THE_LOG =
            listOf(
                "Log.",
                "require(",
                "requireNotNull(",
                "check(",
                "checkNotNull(",
                "error(",
                "Exception(",
                "Error(",
                "testTag(",
            )

        fun of(source: String): List<Prose> = Lexer(source).literals().filter { readsAsProse(it.text) }

        fun readsAsProse(text: String): Boolean {
            val words =
                text.split(Regex("[^\\p{L}']+")).filter { word -> word.count { it.isLetter() } >= 2 }
            val capitalised =
                words.any { word ->
                    val letters = word.filter { it.isLetter() }
                    letters[0].isUpperCase() && letters[1].isLowerCase()
                }
            val spaced = text.any { it.isWhitespace() } && words.isNotEmpty()

            return capitalised || spaced
        }

        private class Lexer(private val source: String) {
            private var at = 0
            private var line = 1
            private val found = mutableListOf<Prose>()
            private val quietUntil = ArrayDeque<Int>()
            private var depth = 0

            fun literals(): List<Prose> {
                code(stopAtBrace = false)
                return found
            }

            private fun peek(offset: Int = 0): Char? = source.getOrNull(at + offset)

            private fun step(): Char {
                val letter = source[at++]
                if (letter == '\n') line++
                return letter
            }

            private fun code(stopAtBrace: Boolean) {
                var braces = 0
                while (at < source.length) {
                    val letter = peek()!!
                    when {
                        source.startsWith("//", at) -> while (at < source.length && peek() != '\n') step()
                        source.startsWith("/*", at) -> {
                            while (at < source.length && !source.startsWith("*/", at)) step()
                            at = minOf(source.length, at + 2)
                        }
                        source.startsWith("\"\"\"", at) -> rawString()
                        letter == '"' -> string()
                        letter == '\'' -> character()
                        letter == '@' -> annotation()
                        letter == '(' -> {
                            step()
                            depth++
                        }
                        letter == ')' -> {
                            step()
                            depth--
                            if (leaveQuiet()) quietLambda()
                        }
                        letter == '{' -> {
                            step()
                            depth++
                            braces++
                        }
                        letter == '}' -> {
                            if (stopAtBrace && braces == 0) {
                                step()
                                return
                            }
                            step()
                            depth--
                            braces--
                            leaveQuiet()
                        }
                        SAID_FOR_THE_LOG.any { source.startsWith(it, at) && !precededByName() } -> {
                            val marker = SAID_FOR_THE_LOG.first { source.startsWith(it, at) }
                            repeat(marker.length - 1) { step() }
                            quietUntil.addLast(depth)
                        }
                        else -> step()
                    }
                }
            }

            private fun leaveQuiet(): Boolean {
                var left = false
                while (quietUntil.isNotEmpty() && quietUntil.last() >= depth) {
                    quietUntil.removeLast()
                    left = true
                }
                return left
            }

            private fun quietLambda() {
                var ahead = at
                while (ahead < source.length && source[ahead] == ' ') ahead++
                if (source.getOrNull(ahead) == '{') quietUntil.addLast(depth)
            }

            private fun precededByName(): Boolean {
                val before = source.getOrNull(at - 1) ?: return false
                val marker = SAID_FOR_THE_LOG.first { source.startsWith(it, at) }
                return marker.first().isLowerCase() && (before.isLetterOrDigit() || before == '_')
            }

            private fun annotation() {
                step()
                while (at < source.length && (peek()!!.isLetterOrDigit() || peek() == '.' || peek() == '_')) step()
                if (peek() == '(') {
                    quietUntil.addLast(depth)
                }
            }

            private fun character() {
                step()
                if (peek() == '\\') step()
                if (at < source.length) step()
                if (peek() == '\'') step()
            }

            private fun string() {
                val started = line
                step()
                val text = StringBuilder()
                while (at < source.length && peek() != '"') {
                    when {
                        peek() == '\\' -> {
                            step()
                            val escaped = step()
                            text.append(if (escaped == 'n') ' ' else escaped)
                        }
                        source.startsWith("\${", at) -> {
                            step()
                            step()
                            text.append("{}")
                            code(stopAtBrace = true)
                        }
                        peek() == '$' && peek(1)?.let { it.isLetter() || it == '_' } == true -> {
                            step()
                            while (at < source.length && (peek()!!.isLetterOrDigit() || peek() == '_')) step()
                            text.append("{}")
                        }
                        else -> text.append(step())
                    }
                }
                if (at < source.length) step()
                keep(started, text.toString())
            }

            private fun rawString() {
                val started = line
                repeat(3) { step() }
                val text = StringBuilder()
                while (at < source.length && !source.startsWith("\"\"\"", at)) {
                    if (source.startsWith("\${", at)) {
                        step()
                        step()
                        text.append("{}")
                        code(stopAtBrace = true)
                    } else {
                        text.append(step())
                    }
                }
                at = minOf(source.length, at + 3)
                keep(started, text.toString())
            }

            private fun keep(started: Int, text: String) {
                if (quietUntil.isEmpty()) found += Prose(started, text)
            }
        }
    }
}
