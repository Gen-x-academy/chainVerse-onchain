#!/usr/bin/env python3
"""Ed25519 signer for the scholarship payout-wallet challenge.

`scholarship-disbursements` gates every payout behind a per-wallet challenge.
`wallet_challenge_payload(challenge_id)` is a public read that returns the
exact 32-byte digest the contract will verify, so the only off-chain step is a
plain Ed25519 signature over those bytes. This module performs that step and
nothing else: it does not reconstruct the contract's signing domain, encode
XDR, or decide which digest is correct.

Scope and safety
----------------
This exists to make the staging seed flow complete using *synthetic* keys. It
is deliberately not a general-purpose wallet:

* It holds no network policy. Stellar secret seeds are StrKey-encoded and a
  testnet secret is indistinguishable from a mainnet one, so refusing "S..."
  would reject every legitimate staging key. The network guard lives in
  `seed-scholarship-staging.sh`, which is the layer that knows the passphrase.
* A wrong signature cannot cause a payout. The contract calls `verify_ed25519`
  and rejects a mismatch with `InvalidSignature`, so the worst outcome of a bug
  here is a failed confirmation, not a wrong one.
* `--self-test` checks this implementation against the RFC 8032 test vectors.
  Run it before trusting a seed.

The implementation follows RFC 8032 (Edwards-Bernstein-DSA) using extended
homogeneous coordinates. It is pure Python because the target environments for
staging do not reliably ship an Ed25519-capable OpenSSL (macOS LibreSSL 3.3
does not) or a Python crypto module.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys

# RFC 8032 / edwards25519 domain parameters.
_Q = 2**255 - 19
_L = 2**252 + 27742317777372353535851937790883648493
_D = -121665 * pow(121666, _Q - 2, _Q) % _Q
_SQRT_M1 = pow(2, (_Q - 1) // 4, _Q)

_Point = tuple  # (X, Y, Z, T) in extended coordinates, or None for identity.


def _sha512(data: bytes) -> bytes:
    return hashlib.sha512(data).digest()


def _recover_x(y: int, sign_bit: int) -> int | None:
    """Recover the x coordinate for a given y, or None if y is not on-curve."""
    if y >= _Q:
        return None
    x2 = (y * y - 1) * pow(_D * y * y + 1, _Q - 2, _Q) % _Q
    if x2 == 0:
        return None if sign_bit else 0
    x = pow(x2, (_Q + 3) // 8, _Q)
    if (x * x - x2) % _Q != 0:
        x = x * _SQRT_M1 % _Q
    if (x * x - x2) % _Q != 0:
        return None
    if x & 1 != sign_bit:
        x = _Q - x
    return x


_BY = 4 * pow(5, _Q - 2, _Q) % _Q
_BX = _recover_x(_BY, 0)
_B = (_BX, _BY, 1, _BX * _BY % _Q)
_IDENTITY: _Point = (0, 1, 1, 0)


def _add(p: _Point, q: _Point) -> _Point:
    x1, y1, z1, t1 = p
    x2, y2, z2, t2 = q
    a = (y1 - x1) * (y2 - x2) % _Q
    b = (y1 + x1) * (y2 + x2) % _Q
    c = t1 * 2 * _D * t2 % _Q
    dd = z1 * 2 * z2 % _Q
    e, f, g, h = b - a, dd - c, dd + c, b + a
    return (e * f % _Q, g * h % _Q, f * g % _Q, e * h % _Q)


def _double(p: _Point) -> _Point:
    x1, y1, z1, _ = p
    a = x1 * x1 % _Q
    b = y1 * y1 % _Q
    c = 2 * z1 * z1 % _Q
    h = a + b
    e = h - (x1 + y1) * (x1 + y1) % _Q
    g = a - b
    f = c + g
    return (e * f % _Q, g * h % _Q, f * g % _Q, e * h % _Q)


def _scalar_mult(p: _Point, e: int) -> _Point:
    result = _IDENTITY
    addend = p
    while e > 0:
        if e & 1:
            result = _add(result, addend)
        addend = _double(addend)
        e >>= 1
    return result


def _encode_point(p: _Point) -> bytes:
    x, y, z, _ = p
    zinv = pow(z, _Q - 2, _Q)
    x = x * zinv % _Q
    y = y * zinv % _Q
    return int.to_bytes(y | ((x & 1) << 255), 32, "little")


def _decode_point(data: bytes) -> _Point:
    if len(data) != 32:
        raise ValueError("point must be 32 bytes")
    y = int.from_bytes(data, "little")
    sign_bit = y >> 255
    y &= (1 << 255) - 1
    x = _recover_x(y, sign_bit)
    if x is None:
        raise ValueError("point is not on the curve")
    return (x, y, 1, x * y % _Q)


def _clamp(h: bytes) -> int:
    a = int.from_bytes(h[:32], "little")
    a &= (1 << 254) - 8
    a |= 1 << 254
    return a


def public_key(seed: bytes) -> bytes:
    """Derive the 32-byte Ed25519 public key for a 32-byte seed."""
    if len(seed) != 32:
        raise ValueError("seed must be 32 bytes")
    h = _sha512(seed)
    return _encode_point(_scalar_mult(_B, _clamp(h)))


def sign(seed: bytes, message: bytes) -> bytes:
    """Return the 64-byte Ed25519 signature (R || S) over `message`."""
    if len(seed) != 32:
        raise ValueError("seed must be 32 bytes")
    h = _sha512(seed)
    a = _clamp(h)
    prefix = h[32:]
    r = int.from_bytes(_sha512(prefix + message), "little") % _L
    big_r = _encode_point(_scalar_mult(_B, r))
    k = int.from_bytes(_sha512(big_r + _encode_point(_scalar_mult(_B, a)) + message), "little") % _L
    s = (r + k * a) % _L
    return big_r + int.to_bytes(s, 32, "little")


def verify(pub: bytes, message: bytes, signature: bytes) -> bool:
    """Verify a signature. Used by the self-test; the contract is the real gate."""
    if len(signature) != 64 or len(pub) != 32:
        return False
    try:
        big_r = _decode_point(signature[:32])
        s = int.from_bytes(signature[32:], "little")
    except ValueError:
        return False
    if s >= _L:
        return False
    k = int.from_bytes(_sha512(signature[:32] + pub + message), "little") % _L
    left = _scalar_mult(_B, s)
    right = _add(big_r, _scalar_mult(_decode_point(pub), k))
    return _encode_point(left) == _encode_point(right)


# RFC 8032 section 7.1 test vectors.
_VECTORS = [
    (
        "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
        "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
        "",
        "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a"
        "33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
    ),
    (
        "4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb",
        "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
        "72",
        "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e"
        "15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00",
    ),
    (
        "c5aa8df43f9f837bedb7442f31dcb7b166d38535076f094b85ce3a2e0b4458f7",
        "fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025",
        "af82",
        "6291d657deec24024827e69c3abe01a30ce548a284743a445e3680d7db5ac3ac18ff9b538d1"
        "6f290ae67f760984dc6594a7c15e9716ed28dc027beceea1ec40a",
    ),
]

_MAINNET_S_SECRET_PREFIX = "S"


def self_test() -> int:
    """Validate against the RFC 8032 vectors. Returns a process exit code."""
    failures = 0
    for i, (seed_h, pub_h, msg_h, sig_h) in enumerate(_VECTORS, start=1):
        seed = bytes.fromhex(seed_h)
        msg = bytes.fromhex(msg_h)
        want_pub = bytes.fromhex(pub_h)
        want_sig = bytes.fromhex(sig_h)
        got_pub = public_key(seed)
        got_sig = sign(seed, msg)
        if got_pub != want_pub:
            print(f"  vector {i}: public key mismatch", file=sys.stderr)
            failures += 1
        if got_sig != want_sig:
            print(f"  vector {i}: signature mismatch", file=sys.stderr)
            failures += 1
        if not verify(want_pub, msg, want_sig):
            print(f"  vector {i}: verify() rejected a valid signature", file=sys.stderr)
            failures += 1
    # A one-bit change must not verify.
    seed = bytes.fromhex(_VECTORS[0][0])
    msg = b"scholarship"
    sig = bytearray(sign(seed, msg))
    sig[0] ^= 0x01
    if verify(public_key(seed), msg, bytes(sig)):
        print("  tampered signature verified", file=sys.stderr)
        failures += 1
    if failures:
        print(f"self-test FAILED ({failures} problem(s))", file=sys.stderr)
        return 1
    print(f"self-test OK ({len(_VECTORS)} RFC 8032 vectors + tamper check)")
    return 0


def _crc16_xmodem(data: bytes) -> int:
    """CRC16-XModem, as used by the Stellar StrKey checksum."""
    crc = 0x0000
    for byte in data:
        crc ^= byte << 8
        for _ in range(8):
            crc = ((crc << 1) ^ 0x1021) & 0xFFFF if crc & 0x8000 else (crc << 1) & 0xFFFF
    return crc


_STRKEY_SECRET_VERSION = 18 << 3  # 0x90; the 6<<3 account version encodes to 'G'
_STRKEY_ACCOUNT_VERSION = 6 << 3  # 0x30; a 'G' address, which is not a secret
_STRKEY_LEN = 56


def secret_to_seed(secret: str) -> bytes:
    """Decode a Stellar StrKey account secret into its raw 32-byte Ed25519 seed.

    `stellar keys secret <name>` prints a StrKey, which is RFC 4648 base32 over
    a version byte, the seed, and a CRC16-XModem checksum. The checksum is
    verified here so a truncated or mistyped secret fails loudly instead of
    producing a wallet nobody controls.
    """
    import base64

    cleaned = secret.strip()
    if len(cleaned) != _STRKEY_LEN:
        raise ValueError(f"a Stellar secret is {_STRKEY_LEN} characters, got {len(cleaned)}")
    try:
        raw = base64.b32decode(cleaned.upper() + "=" * ((8 - len(cleaned) % 8) % 8))
    except Exception as exc:  # noqa: BLE001 - surfaced to the operator
        raise ValueError(f"not valid base32: {exc}") from exc
    if len(raw) != 35:
        raise ValueError(f"expected 35 decoded bytes, got {len(raw)}")
    version, body, checksum = raw[0], raw[1:33], raw[33:35]
    if version != _STRKEY_SECRET_VERSION:
        kind = "an account address" if version == _STRKEY_ACCOUNT_VERSION else "a secret"
        raise ValueError(f"StrKey is {kind}, not a secret "
                         f"(version byte 0x{version:02x})")
    expected = _crc16_xmodem(raw[:33]).to_bytes(2, "little")
    if checksum != expected:
        raise ValueError("checksum mismatch: the secret is mistyped or truncated")
    return body


def seed_to_secret(seed: bytes) -> str:
    """Encode a raw 32-byte seed as a Stellar StrKey. Inverse of `secret_to_seed`."""
    import base64

    if len(seed) != 32:
        raise ValueError("seed must be 32 bytes")
    raw = (bytes([_STRKEY_SECRET_VERSION]) + seed
           + _crc16_xmodem(bytes([_STRKEY_SECRET_VERSION]) + seed).to_bytes(2, "little"))
    return base64.b32encode(raw).decode().lower().rstrip("=")


def _is_hex(value: str) -> bool:
    try:
        bytes.fromhex(value)
    except ValueError:
        return False
    return True


def _decode_seed_argument(value: str) -> bytes:
    import base64

    if _is_hex(value):
        return bytes.fromhex(value)
    return base64.b64decode(value, validate=True)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--self-test", action="store_true",
                        help="run RFC 8032 vectors and exit")
    sub = parser.add_subparsers(dest="command")

    sign_parser = sub.add_parser("sign", help="sign a wallet challenge payload")
    sign_parser.add_argument("--seed", required=True,
                             help="32-byte Ed25519 seed, hex or base64")
    sign_parser.add_argument("--payload", required=True,
                             help="32-byte challenge digest, hex")

    secret_parser = sub.add_parser(
        "secret-to-seed", help="decode a Stellar StrKey secret into a hex seed")
    secret_parser.add_argument("--secret", required=True, help="StrKey account secret")

    encode_parser = sub.add_parser(
        "seed-to-secret", help="encode a hex seed as a Stellar StrKey secret")
    encode_parser.add_argument("--seed", required=True, help="32-byte seed, hex")

    args = parser.parse_args(argv)

    if args.self_test:
        return self_test()

    if args.command == "secret-to-seed":
        try:
            print(secret_to_seed(args.secret).hex())
        except ValueError as exc:
            print(f"error: {exc}", file=sys.stderr)
            return 2
        return 0

    if args.command == "seed-to-secret":
        try:
            print(seed_to_secret(_decode_seed_argument(args.seed)))
        except Exception as exc:  # noqa: BLE001 - surfaced to the operator
            print(f"error: {exc}", file=sys.stderr)
            return 2
        return 0

    if args.command != "sign":
        parser.print_help()
        return 2

    try:
        seed = _decode_seed_argument(args.seed)
    except Exception as exc:  # noqa: BLE001 - surfaced to the operator
        print(f"error: could not decode --seed: {exc}", file=sys.stderr)
        return 2

    if len(seed) != 32:
        print(f"error: seed must be 32 bytes, got {len(seed)}", file=sys.stderr)
        return 2
    if not _is_hex(args.payload) or len(bytes.fromhex(args.payload)) != 32:
        print("error: --payload must be 32 bytes of hex", file=sys.stderr)
        return 2

    signature = sign(seed, bytes.fromhex(args.payload))
    print(json.dumps({
        "pubkey": public_key(seed).hex(),
        "signature": signature.hex(),
        "payload": args.payload.lower(),
    }))


if __name__ == "__main__":
    raise SystemExit(main())
