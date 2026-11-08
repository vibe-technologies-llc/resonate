package org.resonate.android

import android.app.job.JobInfo
import android.app.job.JobParameters
import android.app.job.JobScheduler
import android.app.job.JobService
import android.content.ComponentName
import android.content.Context
import android.util.Log
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import org.resonate.android.nativecore.SyncStanding
import org.resonate.android.nativecore.SyncStatus

enum class BackgroundPass {
    GOING,
    DONE,
    OUT_OF_TIME,
}

data class SyncPass(val status: SyncStatus, val polledAt: Long)

object BackgroundSync {
    const val PERIODIC_JOB = 7101
    const val CARRY_ON_JOB = 7102
    const val WAKES_EVERY_MS = 15 * 60_000L
    const val WAKES_WITHIN_MS = 5 * 60_000L
    const val HEARD_FOR_AT_LEAST_MS = 20_000L
    const val QUIET_FOR_MS = 6_000L
    const val RUNS_AT_MOST_MS = 8 * 60_000L
    const val LOOKED_AT_EVERY_MS = 1_000L

    private val reachingSomeone = setOf(SyncStanding.CONNECTING, SyncStanding.CONNECTED)

    fun quiet(status: SyncStatus): Boolean =
        status.standing == SyncStanding.OFF ||
            (status.moving == 0u && (status.settled || nobodyToMeet(status)))

    private fun nobodyToMeet(status: SyncStatus): Boolean =
        status.standing !in reachingSomeone &&
            status.devices.none { !it.thisDevice && (it.online || it.nearby) }

    fun verdict(status: SyncStatus, ranMs: Long, quietForMs: Long?): BackgroundPass =
        when {
            status.standing == SyncStanding.OFF -> BackgroundPass.DONE
            quietForMs != null &&
                quietForMs >= QUIET_FOR_MS &&
                ranMs >= HEARD_FOR_AT_LEAST_MS -> BackgroundPass.DONE
            ranMs >= RUNS_AT_MOST_MS -> BackgroundPass.OUT_OF_TIME
            else -> BackgroundPass.GOING
        }

    fun pace(context: Context, wakes: Boolean) {
        val scheduler = context.getSystemService(JobScheduler::class.java) ?: return
        if (!wakes) {
            scheduler.cancel(PERIODIC_JOB)
            scheduler.cancel(CARRY_ON_JOB)
            return
        }
        val standing = scheduler.getPendingJob(PERIODIC_JOB)
        if (standing != null && standing.intervalMillis == WAKES_EVERY_MS && standing.isPersisted) {
            return
        }
        val wake =
            JobInfo.Builder(PERIODIC_JOB, ComponentName(context, SyncJobService::class.java))
                .setPeriodic(WAKES_EVERY_MS, WAKES_WITHIN_MS)
                .setRequiredNetworkType(JobInfo.NETWORK_TYPE_ANY)
                .setRequiresBatteryNotLow(true)
                .setPersisted(true)
                .build()
        if (scheduler.schedule(wake) != JobScheduler.RESULT_SUCCESS) {
            Log.w("Resonate", "Device sync could not be woken in the background")
        }
    }

    fun carryOn(context: Context) {
        val scheduler = context.getSystemService(JobScheduler::class.java) ?: return
        if (scheduler.getPendingJob(CARRY_ON_JOB) != null) return
        val carry =
            JobInfo.Builder(CARRY_ON_JOB, ComponentName(context, SyncJobService::class.java))
                .setRequiredNetworkType(JobInfo.NETWORK_TYPE_ANY)
                .build()
        if (scheduler.schedule(carry) != JobScheduler.RESULT_SUCCESS) {
            Log.w("Resonate", "Device sync could not carry on in the background")
        }
    }
}

class SyncJobService : JobService() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val running = HashMap<Int, Job>()

    private val resonateApplication
        get() = application as ResonateApplication

    override fun onStartJob(params: JobParameters): Boolean {
        val library = resonateApplication.library
        running.remove(params.jobId)?.cancel()
        library.retain()
        if (!resonateApplication.current.attached) library.features.foreground(false)
        val pass =
            scope.launch {
                val outcome =
                    try {
                        library.features.syncUntilQuiet()
                    } catch (cancelled: CancellationException) {
                        throw cancelled
                    } catch (failure: Exception) {
                        Log.w("Resonate", "Device sync in the background stopped", failure)
                        BackgroundPass.DONE
                    }
                if (running.remove(params.jobId) != null) {
                    val unfinished =
                        outcome == BackgroundPass.OUT_OF_TIME && library.features.transferring.value
                    jobFinished(params, unfinished)
                }
            }
        pass.invokeOnCompletion { library.release() }
        running[params.jobId] = pass
        return true
    }

    override fun onStopJob(params: JobParameters): Boolean {
        val pass = running.remove(params.jobId) ?: return false
        pass.cancel()
        return resonateApplication.library.features.transferring.value
    }

    override fun onNetworkChanged(params: JobParameters) = Unit

    override fun onDestroy() {
        running.clear()
        scope.cancel()
        super.onDestroy()
    }
}
