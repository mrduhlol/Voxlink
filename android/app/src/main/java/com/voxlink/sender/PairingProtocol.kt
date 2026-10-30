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
    fun claimWithAck(host: String, port: Int = DISCOVERY_PORT, code: String, timeoutMs: Long = 700): Boolean {
        val claim = buildClaim(code)
        val destination = InetAddress.getByName(host)
        DatagramSocket().use { socket ->
            val start = System.nanoTime()
            val deadline = start + timeoutMs * 1_000_000
            var sent = 0
            val bytes = ByteArray(1024)
            while (System.nanoTime() < deadline) {
                if (sent < 5 && System.nanoTime() >= start + sent * 140_000_000L) {
                    socket.send(DatagramPacket(claim, claim.size, destination, port)); sent++
                }
                socket.soTimeout = minOf(40L, maxOf(1L, (deadline - System.nanoTime()) / 1_000_000)).toInt()
                val packet = DatagramPacket(bytes, bytes.size)
                try {
                    socket.receive(packet)
                    if (packet.address == destination && packet.port == port && isAck(bytes, packet.length)) return true
                } catch (_: SocketTimeoutException) { }
            }
        }
        return false
    }
}
