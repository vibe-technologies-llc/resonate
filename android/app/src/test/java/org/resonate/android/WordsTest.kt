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
        val PROPER = setOf("Resonate", "Monochrome", "Subsonic", "Jellyfin", "TIDAL", "Spotify")

        val STILL_TO_MOVE =
            setOf(
                "org/resonate/android/Appearance.kt",
                "org/resonate/android/ArtworkProvider.kt",
                "org/resonate/android/DownloadPlan.kt",
                "org/resonate/android/DownloadQueue.kt",
                "org/resonate/android/DownloadService.kt",
                "org/resonate/android/FindingNotices.kt",
                "org/resonate/android/FolderOrganising.kt",
                "org/resonate/android/FollowedProfileWords.kt",
                "org/resonate/android/LibraryFeatures.kt",
                "org/resonate/android/LibraryRepository.kt",
                "org/resonate/android/LinkedPlaylistWords.kt",
                "org/resonate/android/Listening.kt",
                "org/resonate/android/Logcat.kt",
                "org/resonate/android/Loves.kt",
                "org/resonate/android/MainActivity.kt",
                "org/resonate/android/MusicDocuments.kt",
                "org/resonate/android/OpenedFiles.kt",
                "org/resonate/android/OutputRoutes.kt",
                "org/resonate/android/PartyService.kt",
                "org/resonate/android/PlayerZones.kt",
                "org/resonate/android/ProviderAccountStore.kt",
                "org/resonate/android/ResonateWidgets.kt",
                "org/resonate/android/RoomCorrectionState.kt",
                "org/resonate/android/SafTree.kt",
                "org/resonate/android/SongDeletion.kt",
                "org/resonate/android/SpotifyWords.kt",
                "org/resonate/android/SyncLanding.kt",
                "org/resonate/android/TagEditing.kt",
                "org/resonate/android/TogetherWords.kt",
                "org/resonate/android/ui/Mix.kt",
                "org/resonate/android/ui/Resonate.kt",
                "org/resonate/android/ui/ResonateApp.kt",
                "org/resonate/android/ui/Words.kt",
                "org/resonate/android/ui/components/Banners.kt",
                "org/resonate/android/ui/components/ListenSheet.kt",
                "org/resonate/android/ui/components/Page.kt",
                "org/resonate/android/ui/components/Placeholder.kt",
                "org/resonate/android/ui/components/Reorder.kt",
                "org/resonate/android/ui/components/Rows.kt",
                "org/resonate/android/ui/components/SelectionBar.kt",
                "org/resonate/android/ui/components/Sheets.kt",
                "org/resonate/android/ui/components/TagEditor.kt",
                "org/resonate/android/ui/equaliser/BandEditor.kt",
                "org/resonate/android/ui/equaliser/Controls.kt",
                "org/resonate/android/ui/equaliser/Curves.kt",
                "org/resonate/android/ui/equaliser/Entry.kt",
                "org/resonate/android/ui/equaliser/EqualiserScreen.kt",
                "org/resonate/android/ui/equaliser/Graph.kt",
                "org/resonate/android/ui/equaliser/HeadphoneCorrection.kt",
                "org/resonate/android/ui/equaliser/ListeningTestScreen.kt",
                "org/resonate/android/ui/equaliser/ListeningTestView.kt",
                "org/resonate/android/ui/equaliser/MeasurementAim.kt",
                "org/resonate/android/ui/equaliser/MeasurementScreen.kt",
                "org/resonate/android/ui/equaliser/OutputCurves.kt",
                "org/resonate/android/ui/equaliser/RoomCorrectionScreen.kt",
                "org/resonate/android/ui/equaliser/Scales.kt",
                "org/resonate/android/ui/home/HomeScreen.kt",
                "org/resonate/android/ui/library/AddSongsSheet.kt",
                "org/resonate/android/ui/library/AlbumScreen.kt",
                "org/resonate/android/ui/library/ArtistScreen.kt",
                "org/resonate/android/ui/library/CollectionTabs.kt",
                "org/resonate/android/ui/library/CollectionWords.kt",
                "org/resonate/android/ui/library/FilterSheet.kt",
                "org/resonate/android/ui/library/LibraryScreen.kt",
                "org/resonate/android/ui/library/Menus.kt",
                "org/resonate/android/ui/library/NotHeldRows.kt",
                "org/resonate/android/ui/library/Parts.kt",
                "org/resonate/android/ui/library/PlaylistScreen.kt",
                "org/resonate/android/ui/library/PlaylistViewModel.kt",
                "org/resonate/android/ui/library/SavedSearchDialog.kt",
                "org/resonate/android/ui/library/SavedSearchDraft.kt",
                "org/resonate/android/ui/library/SongCollections.kt",
                "org/resonate/android/ui/library/SongFilter.kt",
                "org/resonate/android/ui/library/SongsTab.kt",
                "org/resonate/android/ui/library/UnheldRelease.kt",
                "org/resonate/android/ui/player/MiniPlayer.kt",
                "org/resonate/android/ui/player/NowPlaying.kt",
                "org/resonate/android/ui/player/PlayerKeys.kt",
                "org/resonate/android/ui/player/QueueSheet.kt",
                "org/resonate/android/ui/player/ZonesSheet.kt",
                "org/resonate/android/ui/search/Downloads.kt",
                "org/resonate/android/ui/search/LibraryResults.kt",
                "org/resonate/android/ui/search/OnlineResults.kt",
                "org/resonate/android/ui/search/OnlineRows.kt",
                "org/resonate/android/ui/search/OnlineTrail.kt",
                "org/resonate/android/ui/search/SearchLogic.kt",
                "org/resonate/android/ui/search/SearchScreen.kt",
                "org/resonate/android/ui/search/WantedScreen.kt",
                "org/resonate/android/ui/server/ServedPages.kt",
                "org/resonate/android/ui/server/ServerParts.kt",
                "org/resonate/android/ui/server/ServerScreen.kt",
                "org/resonate/android/ui/server/ServerWords.kt",
                "org/resonate/android/ui/settings/AppearanceScreen.kt",
                "org/resonate/android/ui/settings/DrivenDevice.kt",
                "org/resonate/android/ui/settings/EnrichmentWords.kt",
                "org/resonate/android/ui/settings/FoldersScreen.kt",
                "org/resonate/android/ui/settings/IdenticalSongsScreen.kt",
                "org/resonate/android/ui/settings/LibraryToolsScreen.kt",
                "org/resonate/android/ui/settings/OrganiseScreen.kt",
                "org/resonate/android/ui/settings/OutputScreen.kt",
                "org/resonate/android/ui/settings/OutputWords.kt",
                "org/resonate/android/ui/settings/ProcessingChoices.kt",
                "org/resonate/android/ui/settings/ProcessingScreen.kt",
                "org/resonate/android/ui/settings/ProviderAccountScreen.kt",
                "org/resonate/android/ui/settings/ProviderWords.kt",
                "org/resonate/android/ui/settings/ProvidersScreen.kt",
                "org/resonate/android/ui/settings/ScrobblingScreen.kt",
                "org/resonate/android/ui/settings/SettingsCatalog.kt",
                "org/resonate/android/ui/settings/SettingsParts.kt",
                "org/resonate/android/ui/settings/SettingsScreen.kt",
                "org/resonate/android/ui/settings/SpotifyScreen.kt",
                "org/resonate/android/ui/settings/StatisticsScreen.kt",
                "org/resonate/android/ui/settings/StatisticsWords.kt",
                "org/resonate/android/ui/settings/SuggestionsScreen.kt",
                "org/resonate/android/ui/settings/SyncScreen.kt",
                "org/resonate/android/ui/settings/SyncWords.kt",
                "org/resonate/android/ui/together/InvitationLink.kt",
                "org/resonate/android/ui/together/PartyScreen.kt",
                "org/resonate/android/ui/together/QrCode.kt",
                "org/resonate/android/ui/together/SharingSheet.kt",
                "org/resonate/android/ui/tools/Analysis.kt",
                "org/resonate/android/ui/tools/Details.kt",
                "org/resonate/android/ui/tools/Lyrics.kt",
                "org/resonate/android/ui/tools/Readings.kt",
                "org/resonate/android/ui/tools/Song.kt",
                "org/resonate/android/ui/tools/Visualizer.kt",
                "org/resonate/android/ui/wrapped/WrappedDeck.kt",
                "org/resonate/android/ui/wrapped/WrappedWords.kt",
            )
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
