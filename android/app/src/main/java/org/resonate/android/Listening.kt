package org.resonate.android

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.resonate.android.nativecore.HeardAnswer
import org.resonate.android.nativecore.HeardSong

class Listening(
    private val library: LibraryRepository,
    private val scope: CoroutineScope,
    private val online: () -> Boolean,
    private val auddToken: () -> String,
) {
    private sealed interface Outcome {
        data class Answered(val answer: HeardAnswer) : Outcome

        data object Denied : Outcome

        data class Failed(val text: String) : Outcome
    }

    private val mutableStage = MutableStateFlow<ListenStage>(ListenStage.Idle)
    private val mutableHeard = MutableStateFlow<List<HeardSong>>(emptyList())
    private val mutableFollowing = MutableStateFlow(ListenFollowing.OFF)
    private val mutableKeeps = MutableStateFlow(false)
    private var running: Job? = null

    val stage: StateFlow<ListenStage> = mutableStage.asStateFlow()
    val heard: StateFlow<List<HeardSong>> = mutableHeard.asStateFlow()
    val following: StateFlow<ListenFollowing> = mutableFollowing.asStateFlow()
    val keepsListening: StateFlow<Boolean> = mutableKeeps.asStateFlow()

    fun listen(allowed: suspend () -> Boolean) {
        if (mutableStage.value.isListening) return
        stopFollowing()
        if (!online()) {
            mutableStage.value = ListenStage.Offline
            return
        }
        mutableStage.value = ListenStage.Recording(1)
        val previous = running
        running =
            scope.launch {
                previous?.join()
                name(allowed)
            }
    }

    fun stop() {
        running?.cancel()
        mutableFollowing.value = ListenFollowing.OFF
        if (mutableStage.value.isListening) mutableStage.value = ListenStage.Idle
    }

    fun keepListening(keeps: Boolean, allowed: suspend () -> Boolean) {
        mutableKeeps.value = keeps
        if (!keeps) {
            stopFollowing()
        } else if (
            !mutableStage.value.isListening &&
                mutableFollowing.value == ListenFollowing.OFF &&
                mutableStage.value.namedOrMissed
        ) {
            val previous = running
            running =
                scope.launch {
                    previous?.join()
                    follow(allowed)
                }
        }
    }

    private fun stopFollowing() {
        if (mutableFollowing.value == ListenFollowing.OFF) return
        running?.cancel()
        mutableFollowing.value = ListenFollowing.OFF
    }

    private suspend fun name(allowed: suspend () -> Boolean) {
        var nth = 1
        while (true) {
            mutableStage.value = ListenStage.Recording(nth)
            val outcome = recordAndAsk(allowed) { mutableStage.value = ListenStage.Asking }
            if (!scope.isActive) return
            when (outcome) {
                Outcome.Denied -> {
                    mutableStage.value = ListenStage.Denied
                    return
                }
                is Outcome.Failed -> {
                    mutableStage.value = ListenStage.Failed(outcome.text)
                    return
                }
                is Outcome.Answered ->
                    when (val next = ListenPlan.after(outcome.answer, nth, null)) {
                        is ListenNext.Tell -> land(next.song)
                        is ListenNext.Again -> {
                            if (!online()) {
                                mutableStage.value = ListenStage.Offline
                                return
                            }
                            nth = next.nth
                            continue
                        }
                        is ListenNext.Settle -> mutableStage.value = next.stage
                        ListenNext.Keep -> Unit
                    }
            }
            break
        }
        follow(allowed)
    }

    private suspend fun follow(allowed: suspend () -> Boolean) {
        while (mutableKeeps.value && mutableStage.value.namedOrMissed) {
            mutableFollowing.value = ListenFollowing.WAITING
            delay(ListenPlan.FOLLOWED_EVERY_SECONDS * 1000)
            if (!mutableKeeps.value) break
            if (!online()) {
                mutableStage.value = ListenStage.Offline
                break
            }
            mutableFollowing.value = ListenFollowing.LISTENING
            when (val outcome = recordAndAsk(allowed) {}) {
                is Outcome.Answered -> {
                    val answer = outcome.answer
                    if (answer is HeardAnswer.Unreached)
                        mutableStage.value = ListenStage.Unreached(answer.service)
                    else {
                        val standing = (mutableStage.value as? ListenStage.Found)?.song
                        val next = ListenPlan.after(answer, null, standing)
                        if (next is ListenNext.Tell) land(next.song)
                    }
                }
                Outcome.Denied -> mutableStage.value = ListenStage.Denied
                is Outcome.Failed -> mutableStage.value = ListenStage.Failed(outcome.text)
            }
        }
        mutableFollowing.value = ListenFollowing.OFF
    }

    private fun land(song: HeardSong) {
        mutableStage.value = ListenStage.Found(song)
        mutableHeard.value = ListenPlan.remembered(mutableHeard.value, song)
    }

    private suspend fun recordAndAsk(
        allowed: suspend () -> Boolean,
        asking: () -> Unit,
    ): Outcome {
        if (!allowed()) return Outcome.Denied
        library.pauseForRecording()
        val clip =
            try {
                Microphone.record(ListenPlan.CLIP_SECONDS)
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failure: Exception) {
                return Outcome.Failed(failure.message ?: "The microphone could not be used")
            }
        asking()
        return try {
            val answer =
                withContext(Dispatchers.IO) {
                    library.native().recogniseHeard(clip.samples.toList(), clip.rate.toUInt(), auddToken())
                }
            Outcome.Answered(answer)
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (failure: Exception) {
            Outcome.Failed(failure.message ?: "The song could not be asked about")
        }
    }
}
