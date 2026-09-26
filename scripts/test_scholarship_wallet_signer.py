#!/usr/bin/env python3
"""Tests for the scholarship payout-wallet challenge signer.

A hand-rolled Ed25519 implementation is only defensible if it is pinned to
known answers, so the RFC 8032 vectors are asserted directly rather than
smoke-tested. The negative cases matter as much as the positive ones: a signer
that accepts a wrong message or a tampered signature would turn the wallet
challenge into decoration.

The contract remains the real gate -- `verify_ed25519` rejects anything
malformed -- so the property under test here is "fails closed", not "approves
payments".
"""

import json
import os
import subprocess
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import scholarship_wallet_signer as signer  # noqa: E402

SCRIPT = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                      "scholarship_wallet_signer.py")

# RFC 8032 section 7.1, (seed, public key, message, signature).
VECTORS = [
    (
        "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
        "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
        "",
        "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555f"
        "b8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
    ),
    (
        "4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb",
        "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
        "72",
        "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da08"
        "5ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00",
    ),
    (
        "c5aa8df43f9f837bedb7442f31dcb7b166d38535076f094b85ce3a2e0b4458f7",
        "fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025",
        "af82",
        "6291d657deec24024827e69c3abe01a30ce548a284743a445e3680d7db5ac3ac18"
        "ff9b538d16f290ae67f760984dc6594a7c15e9716ed28dc027beceea1ec40a",
    ),
]

SEED = bytes.fromhex(VECTORS[0][0])
PUB = bytes.fromhex(VECTORS[0][1])


def run_cli(*args):
    return subprocess.run([sys.executable, SCRIPT, *args],
                          capture_output=True, text=True)


class TestKnownAnswers(unittest.TestCase):
    """Pinned to RFC 8032 so a refactor cannot quietly change the maths."""

    def test_public_keys_match_vectors(self):
        for seed_hex, pub_hex, _, _ in VECTORS:
            with self.subTest(seed=seed_hex[:16]):
                self.assertEqual(signer.public_key(bytes.fromhex(seed_hex)).hex(), pub_hex)

    def test_signatures_match_vectors(self):
        for seed_hex, _, msg_hex, sig_hex in VECTORS:
            with self.subTest(seed=seed_hex[:16]):
                got = signer.sign(bytes.fromhex(seed_hex), bytes.fromhex(msg_hex))
                self.assertEqual(got.hex(), sig_hex)

    def test_empty_message_vector_is_the_hardest_case(self):
        # Vector 1 signs the empty string; a padding or length-prefix bug that
        # only shows up for non-empty input would still pass vectors 2 and 3.
        self.assertEqual(signer.sign(SEED, b"").hex(), VECTORS[0][3])

    def test_signature_is_64_bytes(self):
        self.assertEqual(len(signer.sign(SEED, b"payload")), 64)

    def test_signing_is_deterministic(self):
        self.assertEqual(signer.sign(SEED, b"abc"), signer.sign(SEED, b"abc"))


class TestVerification(unittest.TestCase):
    def test_accepts_valid_signature(self):
        sig = signer.sign(SEED, b"challenge")
        self.assertTrue(signer.verify(PUB, b"challenge", sig))

    def test_rejects_wrong_message(self):
        sig = signer.sign(SEED, b"challenge")
        self.assertFalse(signer.verify(PUB, b"other", sig))

    def test_rejects_tampered_signature(self):
        sig = bytearray(signer.sign(SEED, b"challenge"))
        for index in (0, 31, 32, 63):
            with self.subTest(byte=index):
                mutated = bytearray(sig)
                mutated[index] ^= 0x01
                self.assertFalse(signer.verify(PUB, b"challenge", bytes(mutated)))

    def test_rejects_signature_from_a_different_key(self):
        other = bytes.fromhex(VECTORS[1][0])
        self.assertFalse(signer.verify(PUB, b"challenge", signer.sign(other, b"challenge")))

    def test_rejects_malformed_lengths(self):
        sig = signer.sign(SEED, b"challenge")
        self.assertFalse(signer.verify(PUB, b"challenge", sig[:-1]))
        self.assertFalse(signer.verify(PUB, b"challenge", sig + b"\x00"))
        self.assertFalse(signer.verify(PUB[:-1], b"challenge", sig))

    def test_rejects_non_canonical_scalar(self):
        # An S value at or above the group order is malleable and must be refused.
        group_order = (1 << 252) + 27742317777372353535851937790883648493
        sig = bytearray(signer.sign(SEED, b"challenge"))
        sig[32:] = group_order.to_bytes(32, "little")
        self.assertFalse(signer.verify(PUB, b"challenge", bytes(sig)))


class TestInputValidation(unittest.TestCase):
    def test_rejects_wrong_seed_length(self):
        for bad in (b"", b"\x00" * 31, b"\x00" * 33, b"\x00" * 64):
            with self.subTest(length=len(bad)):
                with self.assertRaises(ValueError):
                    signer.sign(bad, b"x")

    def test_rejects_wrong_point_length(self):
        with self.assertRaises(ValueError):
            signer._decode_point(b"\x00" * 31)

    def test_rejects_off_curve_point(self):
        # y = 2 is not the y of any curve point with a valid x.
        with self.assertRaises(ValueError):
            signer._decode_point((2).to_bytes(32, "little"))


class TestStrKeyCodec(unittest.TestCase):
    """`stellar keys secret` prints a StrKey, so the seed has to come from one.

    A mistyped or truncated secret must fail loudly. If it decoded to a
    plausible-looking seed instead, the staging run would create a payout
    wallet that nobody holds the key for.
    """

    def test_round_trip(self):
        secret = signer.seed_to_secret(SEED)
        self.assertEqual(len(secret), 56)
        self.assertEqual(secret, secret.lower())
        self.assertEqual(signer.secret_to_seed(secret), SEED)

    def test_round_trip_for_several_seeds(self):
        for seed_hex, _, _, _ in VECTORS:
            seed = bytes.fromhex(seed_hex)
            with self.subTest(seed=seed_hex[:16]):
                self.assertEqual(signer.secret_to_seed(signer.seed_to_secret(seed)), seed)

    def test_starts_with_s(self):
        self.assertTrue(signer.seed_to_secret(SEED).startswith("s"))

    def test_rejects_wrong_length(self):
        for bad in ("", "S", "S" * 55, "S" * 57):
            with self.subTest(length=len(bad)):
                with self.assertRaises(ValueError):
                    signer.secret_to_seed(bad)

    def test_rejects_bad_checksum(self):
        secret = signer.seed_to_secret(SEED)
        # Flip a character in the payload region, leaving the checksum stale.
        broken = secret[:20] + ("a" if secret[20] != "a" else "b") + secret[21:]
        with self.assertRaises(ValueError) as ctx:
            signer.secret_to_seed(broken)
        self.assertIn("checksum", str(ctx.exception))

    def test_rejects_non_base32(self):
        with self.assertRaises(ValueError):
            signer.secret_to_seed("!" * 56)

    def test_rejects_public_key_strkey(self):
        # A 'G' account address must be rejected by version, not just by checksum.
        address = signer.seed_to_secret(SEED)
        self.assertTrue(address.startswith("s"))
        with self.assertRaises(ValueError) as ctx:
            signer.secret_to_seed("G" + "A" * 55)
        self.assertIn("not a secret", str(ctx.exception))

    def test_seed_to_secret_rejects_wrong_length(self):
        with self.assertRaises(ValueError):
            signer.seed_to_secret(b"\x00" * 31)


class TestCli(unittest.TestCase):
    def test_self_test_passes(self):
        result = run_cli("--self-test")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("self-test OK", result.stdout)

    def test_sign_emits_json_with_matching_pubkey(self):
        payload = "11" * 32
        result = run_cli("sign", "--seed", SEED.hex(), "--payload", payload)
        self.assertEqual(result.returncode, 0, result.stderr)
        out = json.loads(result.stdout)
        self.assertEqual(out["pubkey"], PUB.hex())
        self.assertEqual(len(bytes.fromhex(out["signature"])), 64)
        self.assertTrue(signer.verify(PUB, bytes.fromhex(payload),
                                      bytes.fromhex(out["signature"])))

    def test_sign_accepts_base64_seed(self):
        import base64
        payload = "22" * 32
        result = run_cli("sign", "--seed", base64.b64encode(SEED).decode(),
                         "--payload", payload)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)["pubkey"], PUB.hex())

    def test_rejects_short_payload(self):
        result = run_cli("sign", "--seed", SEED.hex(), "--payload", "00ff")
        self.assertEqual(result.returncode, 2)

    def test_rejects_non_hex_payload(self):
        result = run_cli("sign", "--seed", SEED.hex(), "--payload", "zz" * 32)
        self.assertEqual(result.returncode, 2)

    def test_secret_to_seed_cli_round_trips(self):
        secret = signer.seed_to_secret(SEED)
        result = run_cli("secret-to-seed", "--secret", secret)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), SEED.hex())

    def test_secret_to_seed_cli_rejects_garbage(self):
        result = run_cli("secret-to-seed", "--secret", "not-a-secret")
        self.assertEqual(result.returncode, 2)
        self.assertIn("error", result.stderr)

    def test_seed_to_secret_cli(self):
        result = run_cli("seed-to-secret", "--seed", SEED.hex())
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), signer.seed_to_secret(SEED))

    def test_bare_invocation_prints_help(self):
        result = run_cli()
        self.assertEqual(result.returncode, 2)
        self.assertIn("usage", result.stdout.lower())


if __name__ == "__main__":
    unittest.main(verbosity=2)
