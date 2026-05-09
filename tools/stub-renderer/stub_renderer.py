#!/usr/bin/env python3
import json
import os
import re
import socket
import struct
import threading
import time
from datetime import datetime, timezone
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from socketserver import ThreadingMixIn, UDPServer
from urllib.parse import urlparse
from xml.sax.saxutils import escape


MEDIA_RENDERER_ST = "urn:schemas-upnp-org:device:MediaRenderer:1"
AV_TRANSPORT_SERVICE = "urn:schemas-upnp-org:service:AVTransport:1"
UUID = os.environ.get("STUB_RENDERER_UUID", "uuid:musicd-stub-renderer")
ACTIONS = [
    "SetAVTransportURI",
    "SetNextAVTransportURI",
    "Play",
    "Pause",
    "Stop",
    "Next",
    "Previous",
    "Seek",
    "GetTransportInfo",
    "GetPositionInfo",
]


def env_int(name, default):
    try:
        return int(os.environ.get(name, str(default)))
    except ValueError:
        return default


HTTP_PORT = env_int("STUB_RENDERER_HTTP_PORT", 9091)
PUBLIC_HOST = os.environ.get("STUB_RENDERER_PUBLIC_HOST", "stub-renderer")
PUBLIC_PORT = env_int("STUB_RENDERER_PUBLIC_PORT", HTTP_PORT)
FRIENDLY_NAME = os.environ.get("STUB_RENDERER_NAME", "musicd Stub Renderer")
PUBLIC_BASE_URL = os.environ.get(
    "STUB_RENDERER_PUBLIC_BASE_URL", f"http://{PUBLIC_HOST}:{PUBLIC_PORT}"
).rstrip("/")


class RendererState:
    def __init__(self):
        self.lock = threading.RLock()
        self.transport_state = os.environ.get("STUB_RENDERER_STATE", "STOPPED")
        self.current_uri = os.environ.get("STUB_RENDERER_CURRENT_URI", "")
        self.next_uri = os.environ.get("STUB_RENDERER_NEXT_URI", "")
        self.position_seconds = env_int("STUB_RENDERER_POSITION_SECONDS", 0)
        self.duration_seconds = env_int("STUB_RENDERER_DURATION_SECONDS", 180)
        self.last_action = None
        self.action_count = 0

    def snapshot(self):
        with self.lock:
            return {
                "transport_state": self.transport_state,
                "current_uri": self.current_uri,
                "next_uri": self.next_uri,
                "position_seconds": self.position_seconds,
                "duration_seconds": self.duration_seconds,
                "last_action": self.last_action,
                "action_count": self.action_count,
                "description_url": f"{PUBLIC_BASE_URL}/description.xml",
                "control_url": f"{PUBLIC_BASE_URL}/upnp/control/AVTransport",
            }

    def update(self, patch):
        allowed = {
            "transport_state",
            "current_uri",
            "next_uri",
            "position_seconds",
            "duration_seconds",
        }
        with self.lock:
            for key, value in patch.items():
                if key not in allowed:
                    continue
                if key in {"position_seconds", "duration_seconds"}:
                    try:
                        value = max(0, int(value))
                    except (TypeError, ValueError):
                        continue
                setattr(self, key, value)
            return self.snapshot()

    def reset(self):
        with self.lock:
            self.transport_state = "STOPPED"
            self.current_uri = ""
            self.next_uri = ""
            self.position_seconds = 0
            self.duration_seconds = env_int("STUB_RENDERER_DURATION_SECONDS", 180)
            self.last_action = None
            self.action_count = 0
            return self.snapshot()

    def record_action(self, action):
        self.last_action = action
        self.action_count += 1


STATE = RendererState()


def seconds_to_upnp(value):
    value = max(0, int(value))
    hours = value // 3600
    minutes = (value % 3600) // 60
    seconds = value % 60
    return f"{hours:02}:{minutes:02}:{seconds:02}"


def upnp_to_seconds(value):
    parts = value.strip().split(":")
    if len(parts) != 3:
        return None
    try:
        hours, minutes, seconds = [int(part) for part in parts]
    except ValueError:
        return None
    return hours * 3600 + minutes * 60 + seconds


def extract_tag(body, tag):
    match = re.search(
        rf"<(?:[^:<>]+:)?{re.escape(tag)}(?:\s[^>]*)?>(.*?)</(?:[^:<>]+:)?{re.escape(tag)}>",
        body,
        re.DOTALL,
    )
    return match.group(1) if match else ""


def action_from_request(headers, body):
    soap_action = headers.get("SOAPACTION", "") or headers.get("Soapaction", "")
    if "#" in soap_action:
        return soap_action.split("#", 1)[1].strip("\"'")
    match = re.search(r"<(?:[^:<>]+:)?([A-Za-z0-9_]+)(?:\s|>)", body)
    if match:
        return match.group(1)
    return ""


def soap_envelope(action, inner):
    return f"""<?xml version="1.0"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
  <s:Body>
    <u:{action}Response xmlns:u="{AV_TRANSPORT_SERVICE}">
      {inner}
    </u:{action}Response>
  </s:Body>
</s:Envelope>"""


def soap_fault(action, code, description):
    return f"""<?xml version="1.0"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
  <s:Body>
    <s:Fault>
      <faultcode>s:Client</faultcode>
      <faultstring>UPnPError</faultstring>
      <detail>
        <UPnPError xmlns="urn:schemas-upnp-org:control-1-0">
          <errorCode>{code}</errorCode>
          <errorDescription>{escape(description)}</errorDescription>
        </UPnPError>
      </detail>
    </s:Fault>
  </s:Body>
</s:Envelope>"""


def description_xml(base_url):
    base_url = base_url.rstrip("/")
    return f"""<?xml version="1.0"?>
<root xmlns="urn:schemas-upnp-org:device-1-0">
  <specVersion>
    <major>1</major>
    <minor>0</minor>
  </specVersion>
  <URLBase>{escape(base_url)}/</URLBase>
  <device>
    <deviceType>{MEDIA_RENDERER_ST}</deviceType>
    <friendlyName>{escape(FRIENDLY_NAME)}</friendlyName>
    <manufacturer>musicd</manufacturer>
    <modelName>Stub Renderer</modelName>
    <UDN>{UUID}</UDN>
    <serviceList>
      <service>
        <serviceType>{AV_TRANSPORT_SERVICE}</serviceType>
        <serviceId>urn:upnp-org:serviceId:AVTransport</serviceId>
        <controlURL>/upnp/control/AVTransport</controlURL>
        <eventSubURL>/upnp/event/AVTransport</eventSubURL>
        <SCPDURL>/upnp/AVTransport.xml</SCPDURL>
      </service>
    </serviceList>
  </device>
</root>"""


def scpd_xml():
    actions = "\n".join(f"      <action><name>{action}</name></action>" for action in ACTIONS)
    return f"""<?xml version="1.0"?>
<scpd xmlns="urn:schemas-upnp-org:service-1-0">
  <specVersion>
    <major>1</major>
    <minor>0</minor>
  </specVersion>
  <actionList>
{actions}
  </actionList>
</scpd>"""


class StubRendererHandler(BaseHTTPRequestHandler):
    server_version = "musicd-stub-renderer/0.1"

    def log_message(self, fmt, *args):
        print(f"[stub-renderer] {self.address_string()} {fmt % args}", flush=True)

    def send_text(self, status, body, content_type):
        encoded = body.encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(encoded)))
        self.end_headers()
        self.wfile.write(encoded)

    def send_json(self, status, payload):
        self.send_text(status, json.dumps(payload, indent=2, sort_keys=True), "application/json")

    def request_base_url(self):
        host = self.headers.get("Host")
        if not host:
            return PUBLIC_BASE_URL
        return f"http://{host}"

    def do_GET(self):
        path = urlparse(self.path).path
        if path == "/description.xml":
            self.send_text(HTTPStatus.OK, description_xml(self.request_base_url()), "application/xml")
        elif path == "/upnp/AVTransport.xml":
            self.send_text(HTTPStatus.OK, scpd_xml(), "application/xml")
        elif path == "/stub/state":
            self.send_json(HTTPStatus.OK, STATE.snapshot())
        elif path == "/health":
            self.send_json(HTTPStatus.OK, {"ok": True})
        else:
            self.send_json(HTTPStatus.NOT_FOUND, {"error": "not found"})

    def do_POST(self):
        path = urlparse(self.path).path
        length = int(self.headers.get("Content-Length", "0") or "0")
        raw_body = self.rfile.read(length)

        if path == "/stub/state":
            try:
                patch = json.loads(raw_body.decode("utf-8") or "{}")
            except json.JSONDecodeError as error:
                self.send_json(HTTPStatus.BAD_REQUEST, {"error": str(error)})
                return
            self.send_json(HTTPStatus.OK, STATE.update(patch))
            return

        if path == "/stub/reset":
            self.send_json(HTTPStatus.OK, STATE.reset())
            return

        if path != "/upnp/control/AVTransport":
            self.send_json(HTTPStatus.NOT_FOUND, {"error": "not found"})
            return

        body = raw_body.decode("utf-8", "replace")
        action = action_from_request(self.headers, body)
        if action not in ACTIONS:
            self.send_text(
                HTTPStatus.INTERNAL_SERVER_ERROR,
                soap_fault(action or "Unknown", 401, f"Invalid action: {action}"),
                "text/xml; charset=utf-8",
            )
            return

        response_body = self.handle_av_transport_action(action, body)
        self.send_text(HTTPStatus.OK, response_body, "text/xml; charset=utf-8")

    def handle_av_transport_action(self, action, body):
        with STATE.lock:
            STATE.record_action(action)
            if action == "SetAVTransportURI":
                STATE.current_uri = extract_tag(body, "CurrentURI")
                STATE.position_seconds = 0
                STATE.transport_state = "STOPPED"
            elif action == "SetNextAVTransportURI":
                STATE.next_uri = extract_tag(body, "NextURI")
            elif action == "Play":
                STATE.transport_state = "PLAYING"
            elif action == "Pause":
                STATE.transport_state = "PAUSED_PLAYBACK"
            elif action == "Stop":
                STATE.transport_state = "STOPPED"
                STATE.position_seconds = 0
            elif action == "Next":
                if STATE.next_uri:
                    STATE.current_uri = STATE.next_uri
                    STATE.next_uri = ""
                    STATE.position_seconds = 0
                STATE.transport_state = "PLAYING"
            elif action == "Previous":
                STATE.position_seconds = 0
            elif action == "Seek":
                parsed = upnp_to_seconds(extract_tag(body, "Target"))
                if parsed is not None:
                    STATE.position_seconds = parsed

            if action == "GetTransportInfo":
                inner = f"""
      <CurrentTransportState>{escape(STATE.transport_state)}</CurrentTransportState>
      <CurrentTransportStatus>OK</CurrentTransportStatus>
      <CurrentSpeed>1</CurrentSpeed>"""
                return soap_envelope(action, inner)

            if action == "GetPositionInfo":
                inner = f"""
      <Track>1</Track>
      <TrackDuration>{seconds_to_upnp(STATE.duration_seconds)}</TrackDuration>
      <TrackMetaData></TrackMetaData>
      <TrackURI>{escape(STATE.current_uri)}</TrackURI>
      <RelTime>{seconds_to_upnp(STATE.position_seconds)}</RelTime>
      <AbsTime>{seconds_to_upnp(STATE.position_seconds)}</AbsTime>
      <RelCount>2147483647</RelCount>
      <AbsCount>2147483647</AbsCount>"""
                return soap_envelope(action, inner)

            return soap_envelope(action, "")


class ReusableUDPServer(ThreadingMixIn, UDPServer):
    allow_reuse_address = True
    daemon_threads = True

    def server_bind(self):
        self.socket.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        try:
            self.socket.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEPORT, 1)
        except (AttributeError, OSError):
            pass
        super().server_bind()


class SsdpHandler:
    def __init__(self, request, client_address, server):
        self.data = request[0]
        self.client_address = client_address
        self.server = server
        self.handle()

    def handle(self):
        request = self.data.decode("utf-8", "ignore")
        upper = request.upper()
        if "M-SEARCH" not in upper or "SSDP:DISCOVER" not in upper:
            return
        if MEDIA_RENDERER_ST not in request and "ssdp:all" not in request.lower():
            return

        date = datetime.now(timezone.utc).strftime("%a, %d %b %Y %H:%M:%S GMT")
        response = "\r\n".join(
            [
                "HTTP/1.1 200 OK",
                "CACHE-CONTROL: max-age=1800",
                f"DATE: {date}",
                "EXT:",
                f"LOCATION: {PUBLIC_BASE_URL}/description.xml",
                "SERVER: musicd-stub/0.1 UPnP/1.1 Python/3",
                f"ST: {MEDIA_RENDERER_ST}",
                f"USN: {UUID}::{MEDIA_RENDERER_ST}",
                "",
                "",
            ]
        )
        self.server.socket.sendto(response.encode("utf-8"), self.client_address)


def run_ssdp():
    server = ReusableUDPServer(("0.0.0.0", 1900), SsdpHandler)
    group = socket.inet_aton("239.255.255.250")
    mreq = group + struct.pack("=I", socket.INADDR_ANY)
    try:
        server.socket.setsockopt(socket.IPPROTO_IP, socket.IP_ADD_MEMBERSHIP, mreq)
    except OSError as error:
        print(f"[stub-renderer] failed to join SSDP multicast group: {error}", flush=True)
    print(
        f"[stub-renderer] SSDP listening on udp/1900 with location {PUBLIC_BASE_URL}/description.xml",
        flush=True,
    )
    server.serve_forever()


def main():
    threading.Thread(target=run_ssdp, daemon=True).start()
    server = ThreadingHTTPServer(("0.0.0.0", HTTP_PORT), StubRendererHandler)
    print(f"[stub-renderer] HTTP listening on 0.0.0.0:{HTTP_PORT}", flush=True)
    print(f"[stub-renderer] description: {PUBLIC_BASE_URL}/description.xml", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
