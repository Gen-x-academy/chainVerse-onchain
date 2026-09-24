# Salted identifier commitments

Tracks issue #1005. Implementation: `contracts/shared/src/commitments.rs`.

## The problem

An ISBN carries roughly 30 bits of entropy and there are only a few million
books in print. A student ID at a given institution is usually six to nine
digits. A library barcode is sequential.

Writing `sha256(identifier)` for any of these to a public ledger is not
privacy. The entire keyspace can be enumerated on a laptop in seconds, so the
digest is equivalent to publishing the plaintext — anyone can build the full
rainbow table and read every record.

## The commitment

```text
commitment = sha256(
      "ChainVerse.Library.Ident"   // 24 bytes, constant
   || kind                          // 4 bytes, big-endian
   || salt_version                  // 4 bytes, big-endian
   || institution_salt              // 32 bytes, secret
   || sha256(identifier)            // 32 bytes
)
```

Total preimage: 96 bytes, every field fixed-width, so two different field sets
can never collide into the same preimage.

The institution salt is the only secret. It is held off-chain; the contract
never sees it and therefore cannot recompute a commitment on a patron's behalf.

### Why 32 bytes of salt

The salt has to defeat an offline dictionary attack over the whole identifier
space. Since the identifier itself contributes ~30 bits, the salt is doing all
the work. 256 bits is the same width as the digest and costs nothing extra.

## Domain separation

Separation happens on three axes at once:

| Axis          | What it prevents                                                  |
|---------------|--------------------------------------------------------------------|
| `kind`        | The same digits being confused across ISBN / barcode / student ID  |
| institution   | Correlating the same person across two institutions                 |
| `salt_version`| An old commitment surviving a rotation                              |

`IdentifierKind::tag()` spells out each numeric tag rather than reading the
enum discriminant, so **reordering the enum cannot silently change every
commitment in existence**.

## Rejecting raw identifiers

`reject_raw_identifier()` screens a value about to be stored or emitted and
rejects anything that looks like plaintext:

* shorter than 16 bytes — too small to be a commitment at all;
* digits only, optionally with `-` or spaces — an ISBN, barcode, or student
  number, regardless of length.

This is a guard rail, not a proof. It cannot detect a high-entropy-*looking*
value that is actually drawn from a small set; that is what the commitment
construction is for. What it does catch is the common mistake of passing the
identifier straight through, which is the failure that actually happens.

## Salt rotation and migration

Rotation is a re-commitment, not an edit. A commitment is a one-way function of
a secret the contract never sees, so the contract **cannot** recompute old
commitments under a new salt. The procedure:

1. The institution generates salt version `n + 1` off-chain.
2. It recomputes every commitment it holds under the new version.
3. It submits the re-commitments alongside the new `salt_version`.
4. The contract accepts both versions during a documented overlap window, then
   drops version `n`.

Because `salt_version` is inside the preimage, a stale commitment simply stops
matching once the overlap closes. There is no path where an old commitment
silently keeps working — `rotating_the_salt_version_re_randomizes_every_commitment`
pins that.

Version `0` is reserved for "unset" and is rejected, so an uninitialised
storage slot can never be mistaken for a valid configuration.

## Generating commitments off-chain

`commitment_preimage()` is `pub` so an institution's tooling can produce
byte-identical commitments without re-implementing the layout. Diff at the
preimage, before the final hash — that is where a mismatch is legible.

## Impact

Additive library code in the `shared` crate. No ABI, storage, event, privacy,
deployment, or migration impact on its own.

Two things **are** breaking when adopted, and each belongs in its own change
with a documented cutover:

* enforcing `reject_raw_identifier()` in an entrypoint that currently accepts a
  raw identifier breaks existing callers;
* rotating a salt invalidates every commitment under the previous version.

`CommitmentError` occupies discriminants 260–264, chosen so they cannot collide
with `ContractError` (1–15), `MathError` (200+), `SigningError` (220+), or
`ConfigError` (240+).
