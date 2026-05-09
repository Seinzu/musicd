package io.musicd.android.data

import android.content.Context
import android.net.wifi.WifiManager
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.IOException
import java.net.DatagramPacket
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.MulticastSocket
import java.net.SocketTimeoutException

data class DiscoveredServer(
    val baseUrl: String,
    val name: String?,
    val location: String?,
    val usn: String?,
)

class MusicdDiscovery(private val context: Context) {
    suspend fun discoverServers(timeoutMillis: Long = DEFAULT_TIMEOUT_MS): List<DiscoveredServer> =
        withContext(Dispatchers.IO) {
            val wifi = context.applicationContext.getSystemService(Context.WIFI_SERVICE) as? WifiManager
            val multicastLock = wifi?.createMulticastLock(MULTICAST_LOCK_TAG)?.apply {
                setReferenceCounted(false)
                runCatching { acquire() }
            }

            try {
                runMSearch(timeoutMillis)
            } finally {
                multicastLock?.runCatching { if (isHeld) release() }
            }
        }

    private fun runMSearch(timeoutMillis: Long): List<DiscoveredServer> {
        val multicastAddress = InetAddress.getByName(SSDP_HOST)
        val request = (
            "M-SEARCH * HTTP/1.1\r\n" +
                "HOST: $SSDP_HOST:$SSDP_PORT\r\n" +
                "MAN: \"ssdp:discover\"\r\n" +
                "MX: 1\r\n" +
                "ST: $MUSICD_SERVER_ST\r\n" +
                "USER-AGENT: musicd-android/1 UPnP/1.1\r\n" +
                "\r\n"
            ).toByteArray(Charsets.US_ASCII)

        val socket = MulticastSocket()
        socket.soTimeout = SOCKET_READ_TIMEOUT_MS
        return try {
            socket.send(DatagramPacket(request, request.size, multicastAddress, SSDP_PORT))

            val deadline = System.currentTimeMillis() + timeoutMillis
            val buffer = ByteArray(8192)
            val seen = LinkedHashMap<String, DiscoveredServer>()

            while (System.currentTimeMillis() < deadline) {
                val packet = DatagramPacket(buffer, buffer.size)
                try {
                    socket.receive(packet)
                } catch (_: SocketTimeoutException) {
                    continue
                } catch (_: IOException) {
                    break
                }
                val response = String(packet.data, packet.offset, packet.length, Charsets.US_ASCII)
                val parsed = parseSsdpResponse(response, packet.address) ?: continue
                seen.putIfAbsent(parsed.baseUrl, parsed)
            }

            seen.values.toList()
        } finally {
            runCatching { socket.close() }
        }
    }

    private fun parseSsdpResponse(response: String, peer: InetAddress): DiscoveredServer? {
        val lines = response.split("\r\n")
        val statusLine = lines.firstOrNull()?.trim().orEmpty()
        if (!statusLine.startsWith("HTTP/1.1 200") && !statusLine.startsWith("HTTP/1.0 200")) {
            return null
        }
        val headers = HashMap<String, String>()
        for (line in lines.drop(1)) {
            val colon = line.indexOf(':')
            if (colon <= 0) continue
            val key = line.substring(0, colon).trim().lowercase()
            val value = line.substring(colon + 1).trim()
            headers[key] = value
        }

        val st = headers["st"].orEmpty()
        val isMusicdSt = st.equals(MUSICD_SERVER_ST, ignoreCase = true) ||
            st.equals(MUSICD_SERVER_ALIAS_ST, ignoreCase = true)
        val hasMusicdHeaders = headers.containsKey("musicd-base-url") || headers.containsKey("musicd-name")
        if (!isMusicdSt && !hasMusicdHeaders) return null

        val advertisedBase = headers["musicd-base-url"]?.takeIf { it.isNotBlank() }
        val location = headers["location"]
        val resolvedBase = resolveBaseUrl(advertisedBase, location, peer) ?: return null

        return DiscoveredServer(
            baseUrl = resolvedBase.trimEnd('/'),
            name = headers["musicd-name"]?.takeIf { it.isNotBlank() },
            location = location,
            usn = headers["usn"],
        )
    }

    private fun resolveBaseUrl(
        advertisedBase: String?,
        location: String?,
        peer: InetAddress,
    ): String? {
        val unusableHosts = setOf("0.0.0.0", "::", "[::]")
        val advertisedHost = advertisedBase?.let { extractHost(it) }
        if (advertisedBase != null && advertisedHost != null && advertisedHost !in unusableHosts) {
            return advertisedBase
        }
        if (location != null) {
            val locationHost = extractHost(location)
            if (locationHost != null && locationHost !in unusableHosts) {
                return location.substringBefore("/description.xml").ifBlank { null }
            }
            val port = extractPort(location)
            if (port != null) {
                return "http://${peer.hostAddress}:$port"
            }
        }
        return null
    }

    private fun extractHost(url: String): String? {
        val schemeEnd = url.indexOf("://").takeIf { it >= 0 } ?: return null
        val rest = url.substring(schemeEnd + 3)
        val authority = rest.substringBefore('/').substringBefore('?')
        val hostPart = if (authority.startsWith('[')) {
            authority.substringAfter('[').substringBefore(']')
        } else {
            authority.substringBefore(':')
        }
        return hostPart.ifBlank { null }
    }

    private fun extractPort(url: String): Int? {
        val schemeEnd = url.indexOf("://").takeIf { it >= 0 } ?: return null
        val rest = url.substring(schemeEnd + 3).substringBefore('/').substringBefore('?')
        val authority = if (rest.startsWith('[')) rest.substringAfter(']') else rest
        val colon = authority.indexOf(':')
        if (colon < 0) return null
        return authority.substring(colon + 1).toIntOrNull()
    }

    companion object {
        const val MUSICD_SERVER_ST = "urn:schemas-musicd-org:device:MusicdServer:1"
        const val MUSICD_SERVER_ALIAS_ST = "musicd:server"
        const val SSDP_HOST = "239.255.255.250"
        const val SSDP_PORT = 1900
        const val DEFAULT_TIMEOUT_MS = 1500L
        private const val SOCKET_READ_TIMEOUT_MS = 250
        private const val MULTICAST_LOCK_TAG = "musicd-discovery"
    }
}
