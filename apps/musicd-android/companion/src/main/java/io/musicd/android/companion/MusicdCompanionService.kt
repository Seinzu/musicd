package io.musicd.android.companion

import android.app.Service
import android.content.Intent
import android.os.IBinder
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineExceptionHandler
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch

class MusicdCompanionService : Service() {
    private lateinit var httpServer: CompanionHttpServer
    private lateinit var playbackRuntime: CompanionPlaybackRuntime
    private lateinit var scanner: LocalLibraryScanner
    private var scanJob: Job? = null
    private val coroutineExceptionHandler = CoroutineExceptionHandler { _, _ -> }
    private val serviceScope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate + coroutineExceptionHandler)

    override fun onCreate() {
        super.onCreate()
        playbackRuntime = CompanionPlaybackRuntime(this)
        playbackRuntime.ensureForegroundStarted()
        scanner = LocalLibraryScanner(this)
        httpServer = CompanionHttpServer(this, playbackRuntime)
        httpServer.start()
    }

    override fun onDestroy() {
        httpServer.stop()
        playbackRuntime.release()
        serviceScope.cancel()
        super.onDestroy()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_START_CONTROL_SURFACE -> {
                playbackRuntime.ensureForegroundStarted()
                playbackRuntime.startControlSurface()
            }
            ACTION_PLAY -> serviceScope.launch { playbackRuntime.playCurrent() }
            ACTION_PAUSE -> serviceScope.launch { playbackRuntime.pause() }
            ACTION_STOP -> serviceScope.launch { playbackRuntime.stop() }
            ACTION_NEXT -> serviceScope.launch { playbackRuntime.next() }
            ACTION_PREVIOUS -> serviceScope.launch { playbackRuntime.previous() }
            ACTION_SCAN_LIBRARY -> startLibraryScan()
            ACTION_CANCEL_SCAN -> cancelLibraryScan()
            else -> {
                playbackRuntime.ensureForegroundStarted()
                playbackRuntime.startControlSurface()
            }
        }
        return START_STICKY
    }

    private fun startLibraryScan() {
        playbackRuntime.ensureForegroundStarted("Scanning music folders")
        if (scanJob != null) return
        scanJob = serviceScope.launch {
            try {
                val summary = scanner.scanAllRoots()
                val title = if (summary.canceled) {
                    "Scan canceled"
                } else {
                    "Scan complete: ${summary.indexed} tracks"
                }
                playbackRuntime.ensureForegroundStarted(title)
            } catch (error: CancellationException) {
                scanner.markActiveScansCanceled()
                playbackRuntime.ensureForegroundStarted("Scan canceled")
            } catch (_: Throwable) {
                playbackRuntime.ensureForegroundStarted("Scan stopped")
            } finally {
                scanJob = null
            }
        }
    }

    private fun cancelLibraryScan() {
        playbackRuntime.ensureForegroundStarted("Canceling scan")
        scanJob?.cancel()
        serviceScope.launch {
            scanner.markActiveScansCanceled()
        }
    }

    override fun onBind(intent: Intent?): IBinder? = null

    companion object {
        const val ACTION_START_CONTROL_SURFACE = "io.musicd.android.companion.START_CONTROL_SURFACE"
        const val ACTION_PLAY = "io.musicd.android.companion.PLAY"
        const val ACTION_PAUSE = "io.musicd.android.companion.PAUSE"
        const val ACTION_STOP = "io.musicd.android.companion.STOP"
        const val ACTION_NEXT = "io.musicd.android.companion.NEXT"
        const val ACTION_PREVIOUS = "io.musicd.android.companion.PREVIOUS"
        const val ACTION_SCAN_LIBRARY = "io.musicd.android.companion.SCAN_LIBRARY"
        const val ACTION_CANCEL_SCAN = "io.musicd.android.companion.CANCEL_SCAN"
    }
}
