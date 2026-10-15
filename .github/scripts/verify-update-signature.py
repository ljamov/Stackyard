"""Checks that an update signature made by `tauri build` matches the app's
public key, so a signing secret that doesn't belong to that key fails the
build instead of shipping updates nobody can install.

Usage: verify-update-signature.py <tauri.conf.json> <file> <file.sig>

Both the public key in tauri.conf.json and the .sig file are base64 of
minisign's text format. Tauri signs the BLAKE2b-512 hash of the file
("ED" signatures); the trusted comment is signed too.
"""

import base64
import hashlib
import json
import sys

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey


def lines(b64):
    text = base64.b64decode(b64).decode()
    return [line for line in text.splitlines() if line.strip()]


def main(conf, file, sig_file):
    with open(conf) as f:
        pub = lines(json.load(f)["plugins"]["updater"]["pubkey"])
    key = base64.b64decode(pub[1])
    if len(key) != 42 or key[:2] != b"Ed":
        sys.exit("the public key in tauri.conf.json is not a minisign key")
    key_id, key = key[2:10], Ed25519PublicKey.from_public_bytes(key[10:])

    with open(sig_file) as f:
        sig = lines(f.read().strip())
    signature = base64.b64decode(sig[1])
    trusted = sig[2].removeprefix("trusted comment: ").encode()
    global_signature = base64.b64decode(sig[3])
    if len(signature) != 74 or signature[:2] != b"ED":
        sys.exit("the update signature is not a prehashed minisign signature")
    if signature[2:10] != key_id:
        sys.exit(
            "the update was signed with key %s, but the app trusts key %s"
            % (signature[2:10][::-1].hex().upper(), key_id[::-1].hex().upper())
        )

    with open(file, "rb") as f:
        digest = hashlib.blake2b(f.read()).digest()
    try:
        key.verify(signature[10:], digest)
        key.verify(global_signature, signature[10:] + trusted)
    except Exception:
        sys.exit("the update signature does not match the app's public key")
    print("Update signature matches key %s" % key_id[::-1].hex().upper())


if __name__ == "__main__":
    main(*sys.argv[1:])
