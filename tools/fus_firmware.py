#!/usr/bin/env python3
"""Retrieve stock Samsung firmware from Samsung's own FUS update service.

Samsung's Smart Switch / Kies clients download firmware from this service. This tool speaks the
same protocol, which is the legitimate route to firmware for a device you own.

Protocol notes, because getting them wrong produces a bare HTTP 401 with no explanation:

* `NF_DownloadGenerateNonce.do` returns a **base64-encoded, AES-CBC encrypted** nonce in the
  `NONCE` response header. It must be *decrypted* first.
* The `Authorization` header's `signature` is that plaintext nonce re-encrypted with a key derived
  from the nonce itself: the first 16 bytes of the key index into a fixed table, the last 16 are a
  constant. Both constants ship in the Smart Switch client and are reproduced by every
  open-source FUS client.
* `LOGIC_CHECK` is a checksum of the firmware version string indexed by the nonce.
* The cloud download endpoint needs the *encrypted* nonce, not the plaintext one.

Usage:
    python3 tools/fus_firmware.py --model SM-A356B --csc EUX
    python3 tools/fus_firmware.py --model SM-A356B --csc EUX --download AP

STATUS: NOT WORKING — `BLOCKED`, and kept only so the next session does not repeat the attempt.

The nonce endpoint (`NF_DownloadGenerateNonce.do`) sometimes answers 200, but the authenticated
binary-information request stays at **401 with no `BINARY_URI`**. The likely cause is that the
`smart-switch`/`kies` client identifiers this tool presents are no longer accepted, or that the
nonce/`LOGIC_CHECK` derivation is wrong for this model; both are indistinguishable from the response
alone, which is why the protocol is documented above rather than left as a guess.

What was tried and did not produce firmware, recorded so it is not retried blindly:

* this FUS client — 401, no `BINARY_URI`;
* `samloader` — `IndexError: string index out of range` in `derive_key`;
* `samfw.com` — Cloudflare Turnstile on every firmware page (a datacentre IP cannot pass it);
* `samfrew.com` / `xdafirmware.com` — 301 to a page that also challenges;
* `opensource.samsung.com` — the mobile list is JavaScript-rendered, so a plain fetch returns 5 KB
  with no model rows.

Conclusion: the A35's own CB app is not obtainable from this container by any route tried, so every
claim about Samsung's gating remains `UNKNOWN`. The analysis that *is* possible without firmware
(carried out instead) exercises the read-only probe against the handset. Do not treat the existence
of this file as evidence that firmware retrieval works.
"""

from __future__ import annotations

import argparse
import base64
import json
import sys
from pathlib import Path
from xml.etree import ElementTree

import requests
from Crypto.Cipher import AES

BASE = "https://neofussvr.sslcs.cdngc.net"
CLOUD = "http://cloud-neofussvr.sslcs.cdngc.net"
USER_AGENT = "Kies2.0_FUS"

# Constants from the Smart Switch client, identical in every open-source FUS implementation.
KEY_1 = b"hqzdurufm2c8mf6bsjezu1qgveouv7c7"
KEY_2 = b"w13r4cvf4hctaujv"


def pkcs_pad(data: bytes) -> bytes:
    return data + bytes([16 - (len(data) % 16)]) * (16 - (len(data) % 16))


def pkcs_unpad(data: bytes) -> bytes:
    return data[:-data[-1]]


def aes_encrypt(data: bytes, key: bytes) -> bytes:
    return AES.new(key, AES.MODE_CBC, key[:16]).encrypt(pkcs_pad(data))


def aes_decrypt(data: bytes, key: bytes) -> bytes:
    return pkcs_unpad(AES.new(key, AES.MODE_CBC, key[:16]).decrypt(data))


def decrypt_nonce(encrypted: str) -> str:
    """Normalise the server's NONCE header to plaintext.

    The endpoint has returned it two ways: as base64 of an AES-CBC ciphertext, and more recently as
    the plaintext itself. Length decides — a base64 body decodes to a multiple of the AES block size
    and does not, while a 16-character plaintext nonce is already printable ASCII.
    """
    candidate = encrypted.strip()
    try:
        raw = base64.b64decode(candidate, validate=True)
    except Exception:  # noqa: BLE001 - not base64 at all, so it is the plaintext
        return candidate
    if raw and len(raw) % 16 == 0 and candidate != raw.decode("utf-8", "replace"):
        try:
            plain = aes_decrypt(raw, KEY_1)
            if plain.isascii() and plain.isprintable():
                return plain.decode()
        except Exception:  # noqa: BLE001 - fall through to treating it as plaintext
            pass
    return candidate


def derive_key(nonce: str) -> bytes:
    """KEY_1 indexes are bytes, so build the key as bytes rather than decoding it."""
    return bytes(KEY_1[ord(nonce[i]) % 16] for i in range(16)) + KEY_2


def get_auth(nonce: str) -> str:
    return base64.b64encode(aes_encrypt(nonce.encode(), derive_key(nonce))).decode()


def logic_check(version: str, nonce: str) -> str:
    if len(version) < 16:
        raise ValueError(f"version too short for LOGIC_CHECK: {version!r}")
    return "".join(version[ord(c) & 0xF] for c in nonce)


class FusClient:
    def __init__(self) -> None:
        self.session = requests.Session()
        self.enc_nonce = ""
        self.nonce = ""
        self.auth = ""
        self._request("NF_DownloadGenerateNonce.do")

    def _request(self, path: str, body: str = "") -> str:
        auth_header = (
            f'FUS nonce="", signature="{self.auth}", nc="", type="", realm="", newauth="1"'
        )
        response = self.session.post(
            f"{BASE}/{path}",
            data=body,
            headers={"Authorization": auth_header, "User-Agent": USER_AGENT},
            timeout=60,
        )
        if "NONCE" in response.headers:
            self.enc_nonce = response.headers["NONCE"]
            self.nonce = decrypt_nonce(self.enc_nonce)
            self.auth = get_auth(self.nonce)
        response.raise_for_status()
        return response.text

    def inform(self, model: str, region: str, version: str) -> dict:
        body = _xml(
            {
                "ACCESS_MODE": "2",
                "BINARY_NATURE": "1",
                "CLIENT_PRODUCT": "Smart Switch",
                "CLIENT_VERSION": "4.3.23073_1",
                "DEVICE_FW_VERSION": version,
                "DEVICE_LOCAL_CODE": region,
                "DEVICE_MODEL_NAME": model,
                "LOGIC_CHECK": logic_check(version, self.nonce),
            }
        )
        text = self._request("NF_DownloadBinaryInform.do", body)
        root = ElementTree.fromstring(text)
        out: dict[str, str] = {}
        for element in root.iter():
            data = element.find("Data")
            if data is not None and data.text is not None:
                out[element.tag] = data.text
        for status in root.iter():
            if status.tag in {"Status", "Code", "Reason"}:
                out[f"_status.{status.tag}"] = status.text or ""
        return out

    def download(self, filename: str, destination: Path, start: int = 0) -> None:
        auth_header = (
            f'FUS nonce="{self.enc_nonce}", signature="{self.auth}", nc="", '
            f'type="", realm="", newauth="1"'
        )
        headers = {"Authorization": auth_header, "User-Agent": USER_AGENT}
        if start:
            headers["Range"] = f"bytes={start}-"
        response = self.session.get(
            f"{CLOUD}/NF_DownloadBinaryForMass.do",
            params={"file": filename},
            headers=headers,
            stream=True,
            timeout=120,
        )
        response.raise_for_status()
        mode = "ab" if start else "wb"
        with destination.open(mode) as handle:
            for chunk in response.iter_content(chunk_size=1 << 20):
                handle.write(chunk)


def _xml(fields: dict[str, str]) -> str:
    entries = "".join(f"<{k}><Data>{v}</Data></{k}>" for k, v in fields.items())
    return (
        "<FUSMsg><FUSHdr><ProtoVer>1.0</ProtoVer></FUSHdr>"
        f"<FUSBody><Put>{entries}</Put></FUSBody></FUSMsg>"
    )


def version_from_fota(model: str, region: str) -> str | None:
    """Ask Samsung's FOTA service for the latest version string, if this network can reach it."""
    try:
        response = requests.get(
            f"https://fota-cloud-dn.ospserver.net/firmware/{region}/{model}/version.xml",
            timeout=30,
        )
        if response.status_code != 200:
            return None
        root = ElementTree.fromstring(response.text)
        latest = root.find("./firmware/version/latest")
        if latest is None or not latest.text:
            return None
        parts = latest.text.split("/")
        if len(parts) == 3:
            parts.append(parts[0])
        if len(parts) > 2 and parts[2] == "":
            parts[2] = parts[0]
        return "/".join(parts)
    except Exception:  # noqa: BLE001 - absence of FOTA is not an error, it just removes a hint
        return None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model", default="SM-A356B")
    parser.add_argument("--csc", default="EUX")
    parser.add_argument("--version", default=None,
                        help="firmware version string; omit to try the FOTA hint")
    parser.add_argument("--download", default=None, choices=["AP", "BL", "CP", "CSC", "HOME_CSC"])
    parser.add_argument("--out", default="/tmp/fw")
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()

    client = FusClient()
    version = args.version
    if version is None:
        version = version_from_fota(args.model, args.csc)
        if version:
            print(f"FOTA reports latest version: {version}", file=sys.stderr)

    info: dict = {}
    tried: list[str] = []
    for candidate in [version, "", f"{args.model}/{args.csc}"]:
        if candidate is None or candidate in tried:
            continue
        tried.append(candidate)
        try:
            info = client.inform(args.model, args.csc, candidate)
        except Exception as exc:  # noqa: BLE001
            print(f"inform with version {candidate!r} failed: {exc}", file=sys.stderr)
            continue
        if info.get("BINARY_URI"):
            break

    if args.json:
        print(json.dumps(info, indent=2, sort_keys=True))
    else:
        for key in (
            "DEVICE_MODEL_DISPLAY_NAME", "DEVICE_LOCAL_CODE", "LATEST_FW_VERSION",
            "CURRENT_OS_VERSION", "CURRENT_DISPLAY_VERSION", "BINARY_URI", "BINARY_NAME",
            "BINARY_BYTE_SIZE", "LAST_MODIFIED", "DESCRIPTION",
        ):
            if key in info:
                print(f"{key:26} {info[key]}")
        if "_status.Status" in info:
            print(f"{'STATUS':26} {info['_status.Status']}")

    uri = info.get("BINARY_URI")
    if not uri:
        print("\nNo BINARY_URI returned; the server offered no firmware for this model/region.",
              file=sys.stderr)
        return 1

    if args.download:
        destination = Path(args.out)
        destination.mkdir(parents=True, exist_ok=True)
        target = destination / Path(uri).name
        print(f"downloading {uri} -> {target}", file=sys.stderr)
        client.download(uri, target)
        print(f"wrote {target} ({target.stat().st_size} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
