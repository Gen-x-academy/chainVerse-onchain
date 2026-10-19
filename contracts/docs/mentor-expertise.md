# Mentor Expertise Taxonomy

- Status: Implemented
- Related issue: MENTOR-010 (Identity track)

## Scope

`mentor-expertise` manages the public vocabulary used to classify mentor
expertise. Admins register canonical skill IDs, immutable parent links, public
labels, aliases, proficiency-specific evidence requirement commitments, and
deprecations with optional replacement terms.

## Taxonomy and bounds

- Skills are identified by nonzero `BytesN<32>` IDs. Root terms have no
  parent; children require an existing active parent. Parent links never
  change, preventing cycles by construction.
- Maximum hierarchy depth is 8, skill count is 512, alias count is 2,048,
  and `list_skills` returns at most 50 records per call.
- Registry records use persistent storage with a 1–2 year TTL; consumers that
  need long-lived entries should periodically read them to renew their TTL.
  Monotonic registry/version counters live in instance storage and do not reset
  when a persistent record expires.
- Public aliases are unique and cannot be reused after deprecation.
- Proficiency levels are `Foundation`, `Developing`, `Proficient`, and
  `Expert`. Evidence requirements are hash commitments with a nonzero schema
  version and immutable per-level history.
- Deprecated skills remain readable for historical references. New children,
  aliases, and evidence requirements cannot be attached to deprecated skills.
  An optional replacement must be a different active skill.

## Privacy and authorization

Only public taxonomy labels/aliases, opaque skill IDs, status, timestamps,
admin addresses, and hashes of off-chain evidence criteria are stored. Mentor
profiles, credentials, evidence files, and detailed rubrics remain off-chain.
All mutations require the initialized admin's authorization; reads are public.

## Events

`SKNEW` records a skill registration, `SKALIAS` an alias mapping, `ALIASDEP`
an alias retirement, `SKDEPR` a skill deprecation/replacement, and `EVIDREQ` a
new evidence requirement version.

## Validation

Run `cargo test -p mentor-expertise` from `contracts/`.