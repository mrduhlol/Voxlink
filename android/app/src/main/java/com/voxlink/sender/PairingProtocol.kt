package com.voxlink.sender

import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.Inet4Address
import java.net.InetAddress
import java.net.NetworkInterface
import java.net.SocketTimeoutException
import java.net.URI
import java.util.Collections

object PairingProtocol {
    const val DISCOVERY_PORT = 4951
    const val LOCAL_ADDRESS_ERROR = "That address is this phone (or localhost). Use your computer's Wi-Fi IP — the PC shows it, or tap Find my computer."
    data class Reply(val host: String, val audioPort: Int, val code: String)
    data class PairingTarget(val host: String, val port: Int, val code: String)
    private fun validCode(code: String) = code.length == 6 && code.all { it in '0'..'9' }
    private fun ipv4(host: String): List<Int>? {
        val parts = host.split('.')
        if (parts.size != 4 || parts.any { it.isEmpty() || it.length > 3 || it.any { c -> c !in '0'..'9' } }) return null
        val octets = parts.map { it.toInt() }
        return octets.takeIf { it.all { n -> n in 0..255 } }
    }
    fun buildProbe() = "VXLD".toByteArray(Charsets.US_ASCII)
    fun buildClaim(code: String): ByteArray {
        require(validCode(code)) { "Enter the six-digit pairing code shown on the PC." }
        return "VXLC$code".toByteArray(Charsets.US_ASCII)
    }
    fun parseReply(data: ByteArray, length: Int, fromHost: String): Reply? {
        if (length != 12 || data.size < length || String(data, 0, 4, Charsets.US_ASCII) != "VXLR") return null
        val code = String(data, 4, 6, Charsets.US_ASCII)
        val port = ((data[10].toInt() and 255) shl 8) or (data[11].toInt() and 255)
        return if (validCode(code) && port in 1024..65535 && port != DISCOVERY_PORT) Reply(fromHost, port, code) else null
    }
    fun isAck(data: ByteArray, length: Int) = length == 4 && data.size >= 4 && String(data, 0, 4, Charsets.US_ASCII) == "VXLA"
    fun isPending(data: ByteArray, length: Int) = length == 4 && data.size >= 4 && String(data, 0, 4, Charsets.US_ASCII) == "VXLP"
    fun isRejected(data: ByteArray, length: Int) = length == 4 && data.size >= 4 && String(data, 0, 4, Charsets.US_ASCII) == "VXLN"
    fun parsePairingUri(uri: String): PairingTarget? = try {
        val parsed = URI(uri)
        val host = parsed.host ?: ""
        val code = parsed.rawQuery?.takeIf { it.matches(Regex("code=[0-9]{6}")) }?.substringAfter('=')
        if (parsed.scheme != "voxlink" || ipv4(host) == null || validateDestination(host) != null ||
            parsed.port !in 1024..65535 || parsed.port == DISCOVERY_PORT || code == null ||
            parsed.rawUserInfo != null || !parsed.rawPath.isNullOrEmpty() || parsed.rawFragment != null) null
        else PairingTarget(host, parsed.port, code)
    } catch (_: Exception) { null }

    fun localAddresses(): List<InetAddress> = try {
        Collections.list(NetworkInterface.getNetworkInterfaces()).filter { it.isUp && !it.isLoopback }
            .flatMap { Collections.list(it.inetAddresses) }.filterIsInstance<Inet4Address>()
    } catch (_: Exception) { emptyList() }

    fun validateDestination(host: String): String? {
        if (host.equals("localhost", true)) return LOCAL_ADDRESS_ERROR
        val ip = ipv4(host) ?: return "Enter your computer's Wi-Fi IPv4 address, or tap Find my computer."
        if (ip[0] == 127 || ip[0] == 0 || (ip[0] == 169 && ip[1] == 254) ||
            localAddresses().any { it.hostAddress == host }) return LOCAL_ADDRESS_ERROR
        if (ip[0] >= 224 || host == "255.255.255.255") return "Use your computer's Wi-Fi IPv4 address, not a broadcast or multicast address."
        return null
    }
    fun subnetBroadcastOf(address: InetAddress): String? {
        if (address !is Inet4Address || !address.isSiteLocalAddress) return null
        return address.hostAddress?.substringBeforeLast('.') + ".255"
    }
    fun discover(targets: List<String>, port: Int = DISCOVERY_PORT, windowMs: Long = 1200): List<Reply> {
        val replies = linkedMapOf<String, Reply>()
        val destinations = targets.distinct().mapNotNull { try { InetAddress.getByName(it) } catch (_: Exception) { null } }
        DatagramSocket().use { socket ->
            socket.broadcast = true
            val start = System.nanoTime()
            val deadline = start + windowMs * 1_000_000
            var burst = 0
            val bytes = ByteArray(1024)
            while (System.nanoTime() < deadline) {
                val now = System.nanoTime()
                if (burst < 4 && now >= start + burst * 250_000_000L) {
                    val probe = buildProbe()
                    destinations.forEach { destination ->
                        try { socket.send(DatagramPacket(probe, probe.size, destination, port)) } catch (_: java.io.IOException) { }
                    }
                    burst++
                }
                socket.soTimeout = minOf(50L, maxOf(1L, (deadline - System.nanoTime()) / 1_000_000)).toInt()
                val packet = DatagramPacket(bytes, bytes.size)
                try {
                    socket.receive(packet)
                    if (packet.port != port) continue
                    parseReply(bytes, packet.length, packet.address.hostAddress ?: "")?.let { replies[it.host] = it }
                } catch (_: SocketTimeoutException) { }
            }
        }
        return replies.values.toList()
    }
    /** Outcome of a pairing request against the PC approval contract. */
    enum class PairingDecision { APPROVED, REJECTED, TIMEDOUT, CANCELLED }

    const val PAIRING_TIMEOUT_MS = 45_000L

    /**
     * Send VXLC<code> claims and wait for the PC's approval decision (VXLA approved / VXLN
     * rejected / VXLP pending), retrying the claim periodically for at most [timeoutMs].
     *
     * A valid code alone is never approval: this returns REJECTED, TIMEDOUT, or CANCELLED
     * until the PC answers VXLA. [shouldContinue] aborts early (Stop / activity stopped)
     * with CANCELLED. [onPending] fires on the transition into the PC's "waiting for
     * approval" state (first VXLP only, not on every retry).
     */
    fun requestPairing(
        host: String,
        port: Int = DISCOVERY_PORT,
        code: String,
        timeoutMs: Long = PAIRING_TIMEOUT_MS,
        shouldContinue: () -> Boolean = { true },
        onPending: () -> Unit = { },
    ): PairingDecision {
        val claim = buildClaim(code)
        val destination = InetAddress.getByName(host)
        DatagramSocket().use { socket ->
            val deadline = System.nanoTime() + timeoutMs * 1_000_000
            var claimsSent = 0
            var nextClaimAt = 0L
            var pendingAnnounced = false
            val bytes = ByteArray(1024)
            while (true) {
                if (!shouldContinue()) return PairingDecision.CANCELLED
                val now = System.nanoTime()
                if (now >= deadline) return PairingDecision.TIMEDOUT
                // Fast triple burst first so a legacy/immediate VXLA is not delayed, then a
                // steady retry every 700 ms so the PC keeps seeing the claim while its
                // approval pop-up is open (VXLP pending) or was missed entirely.
                if (now >= nextClaimAt) {
                    socket.send(DatagramPacket(claim, claim.size, destination, port))
                    claimsSent++
                    nextClaimAt = now + (if (claimsSent < 3) 140L else 700L) * 1_000_000
                }
                socket.soTimeout = minOf(40L, maxOf(1L, (deadline - System.nanoTime()) / 1_000_000)).toInt()
                val packet = DatagramPacket(bytes, bytes.size)
                try {
                    socket.receive(packet)
                } catch (_: SocketTimeoutException) { continue }
                if (packet.address != destination || packet.port != port) continue
                if (isAck(bytes, packet.length)) return PairingDecision.APPROVED
                if (isPending(bytes, packet.length)) {
                    // Announce only the transition into "waiting for PC approval": the
                    // desktop answers VXLP to every retried claim, so firing per packet
                    // would flood and mislead — no state has changed.
                    if (!pendingAnnounced) {
                        pendingAnnounced = true
                        onPending()
                    }
                    // While the PC reports pending, keep claiming every 700 ms in case the
                    // desktop restarts or wants to refresh its approval pop-up from the claim.
                    nextClaimAt = System.nanoTime() + 700L * 1_000_000
                } else if (isRejected(bytes, packet.length)) return PairingDecision.REJECTED
                // Ignore anything else (VXLR discovery replies, garbage, malformed packets).
            }
        }
    }

    /**
     * Legacy/test helper: true only when the PC answers VXLA immediately, false on VXLP
     * pending, VXLN rejection, or silence. Delegates to [requestPairing].
     */
    fun claimWithAck(host: String, port: Int = DISCOVERY_PORT, code: String, timeoutMs: Long = 700): Boolean =
        try { requestPairing(host, port, code, timeoutMs) == PairingDecision.APPROVED } catch (_: java.io.IOException) { false }
}
