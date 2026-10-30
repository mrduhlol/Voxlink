package com.voxlink.sender

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Bundle
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView
import android.widget.Toast
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageProxy
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.LifecycleRegistry
import com.google.zxing.BinaryBitmap
import com.google.zxing.MultiFormatReader
import com.google.zxing.PlanarYUVLuminanceSource
import com.google.zxing.common.HybridBinarizer
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

class ScannerActivity : Activity(), LifecycleOwner {
    private val registry = LifecycleRegistry(this)
    override fun getLifecycle(): Lifecycle = registry
    private val executor = Executors.newSingleThreadExecutor()
    private val found = AtomicBoolean(false)
    private lateinit var preview: PreviewView
    private var provider: ProcessCameraProvider? = null
    private var lastInvalid = 0L

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        registry.handleLifecycleEvent(Lifecycle.Event.ON_CREATE)
        val root = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; setPadding(24, 24, 24, 24) }
        root.addView(TextView(this).apply { text = "Scan the QR code shown in Voxlink on your computer."; textSize = 18f }, LinearLayout.LayoutParams(-1, -2))
        preview = PreviewView(this)
        root.addView(preview, LinearLayout.LayoutParams(-1, 0, 1f))
        root.addView(Button(this).apply { text = "Cancel"; setOnClickListener { finish() } })
        setContentView(root)
        if (checkSelfPermission(Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED) startCamera()
        else requestPermissions(arrayOf(Manifest.permission.CAMERA), 1002)
    }
    override fun onStart() { super.onStart(); registry.handleLifecycleEvent(Lifecycle.Event.ON_START) }
    override fun onResume() { super.onResume(); registry.handleLifecycleEvent(Lifecycle.Event.ON_RESUME) }
    override fun onPause() { registry.handleLifecycleEvent(Lifecycle.Event.ON_PAUSE); super.onPause() }
    override fun onStop() { registry.handleLifecycleEvent(Lifecycle.Event.ON_STOP); super.onStop() }
    override fun onDestroy() {
        provider?.unbindAll()
        executor.shutdown()
        registry.handleLifecycleEvent(Lifecycle.Event.ON_DESTROY)
        super.onDestroy()
    }
    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode == 1002) {
            if (grantResults.firstOrNull() == PackageManager.PERMISSION_GRANTED) startCamera()
            else { Toast.makeText(this, "Camera permission denied. Use Find my computer or enter the PC address.", Toast.LENGTH_LONG).show(); finish() }
        }
    }
    private fun startCamera() {
        val future = ProcessCameraProvider.getInstance(this)
        future.addListener({
            if (isFinishing || isDestroyed) return@addListener
            try {
                val camera = future.get()
                provider = camera
                val view = androidx.camera.core.Preview.Builder().build().also { it.setSurfaceProvider(preview.surfaceProvider) }
                val analysis = ImageAnalysis.Builder().setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST).build()
                analysis.setAnalyzer(executor) { image -> decode(image) }
                camera.bindToLifecycle(this, CameraSelector.DEFAULT_BACK_CAMERA, view, analysis)
            } catch (_: Exception) {
                Toast.makeText(this, "Camera unavailable. Use Find my computer or enter the PC address.", Toast.LENGTH_LONG).show()
                finish()
            }
        }, java.util.concurrent.Executor { command -> runOnUiThread(command) })
    }
    private fun decode(image: ImageProxy) {
        try {
            if (found.get()) return
            val plane = image.planes[0]
            val buffer = plane.buffer
            val bytes = ByteArray(image.width * image.height)
            val base = buffer.position()
            for (y in 0 until image.height) for (x in 0 until image.width) {
                bytes[y * image.width + x] = buffer.get(base + y * plane.rowStride + x * plane.pixelStride)
            }
            val source = PlanarYUVLuminanceSource(bytes, image.width, image.height, 0, 0, image.width, image.height, false)
            val result = MultiFormatReader().decode(BinaryBitmap(HybridBinarizer(source))).text
            if (PairingProtocol.parsePairingUri(result) != null && found.compareAndSet(false, true)) {
                runOnUiThread { setResult(RESULT_OK, Intent().putExtra("pairingUri", result)); finish() }
            } else if (System.nanoTime() - lastInvalid > 2_000_000_000L) {
                lastInvalid = System.nanoTime()
                runOnUiThread { Toast.makeText(this, "Not a Voxlink QR code", Toast.LENGTH_SHORT).show() }
            }
        } catch (_: Exception) { /* No readable QR in this frame. */ }
        finally { image.close() }
    }
}
