#!/usr/bin/env python3
"""Parses EncodeDump's output exactly the way Pumpkin's velocity.rs does and checks every field."""
import hashlib, hmac, io, sys, uuid

def varint(r):
    n = s = 0
    while True:
        b = r.read(1)[0]
        n |= (b & 0x7f) << s
        if not b & 0x80: return n
        s += 7
def string(r): return r.read(varint(r)).decode()

raw = bytes.fromhex(sys.stdin.read().strip())
r = io.BytesIO(raw)
assert varint(r) == 0x02, "packet id must be custom_query_answer"
assert varint(r) == 123456, "message id"
assert r.read(1) == b"\x01", "understood flag"
data = r.read()
sig, body = data[:32], data[32:]
assert hmac.compare_digest(sig, hmac.new(b"s3cret", body, hashlib.sha256).digest()), "HMAC"
r = io.BytesIO(body)
assert varint(r) == 1, "forwarding version"
assert string(r) == "2001:db8::1", "address"
assert uuid.UUID(bytes=r.read(16)) == uuid.UUID("069a79f4-44e9-4726-a5be-fca90e38aaf5"), "uuid"
assert string(r) == "Notch", "name"
props = []
for _ in range(varint(r)):
    name, value = string(r), string(r)
    sig = string(r) if r.read(1) == b"\x01" else None
    props.append((name, value, sig))
assert props == [("textures", "dGV4dHVyZXM=", "c2ln"), ("other", "x", None)], props
assert r.read() == b"", "trailing bytes"
print("OK: forwarding packet parses exactly as Pumpkin expects")
