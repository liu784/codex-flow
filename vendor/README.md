# Temporary Tauri dependency patches

The Windows release locks two upstream crates that still depend on SHA-2 0.10,
which brings in the affected `block-buffer` 0.10.4. The upstream releases below
were downloaded from crates.io and verified against their original Cargo.lock
checksums before these small dependency changes:

- `tauri-codegen` 2.6.3, original SHA-256: `08279169ff42f8fc45a1dbc9dcae888893ba95288142e5880c59b93a26d2cfc5`
- `wry` 0.55.1, original SHA-256: `186f9871daa55fd9c016578b810d149de58367113db7fb72b462d2323ce19514`

Only their `sha2` dependency specifications were changed from 0.10 to 0.11;
the latter resolves `block-buffer` 0.12.1, which fixes
[GHSA-qwgh-2vcv-g2f7](https://github.com/advisories/GHSA-qwgh-2vcv-g2f7).
Their bundled license files remain in each directory. The original nested
upstream lockfiles were removed to avoid stale, unused dependency locks.

These local patches pin the upstream crate versions. When Tauri's released
versions use a fixed dependency, remove the `[patch.crates-io]` entries and
these copies, then update `Cargo.lock`, repeat the security scan and tests.
