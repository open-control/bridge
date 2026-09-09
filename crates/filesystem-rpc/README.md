# Filesystem wire contract v2 — R2 prototype

This unpublished, dependency-free crate owns only the borrowed wire envelope.
Core implements the same contract in `UnifiedFileSystemRpc.{hpp,cpp}`. It does
not translate requests into the previous named filesystem/job frames.

The application and Manager UI still use the previous protocol. This isolated
prototype is not a released compatibility promise. Operation bodies beyond the
R2 subset below are reserved for the coordinated migration.

## Envelope

All integers are little-endian. A frame is exactly 24 bytes plus its body; no
padding or trailing bytes are accepted. Maximum body length is 32,512 bytes.

| Offset | Type | Meaning |
| --- | --- | --- |
| 0 | u8 | `0xfc` request, `0xfd` response |
| 1 | u8 | Version, exactly `2` |
| 2 | u8 | Operation, 0 through 14 |
| 3 | u8 | State in bits 0–6; bit 7 marks a retained mutation replay |
| 4 | u16 | Nonzero transport request ID, echoed in the response |
| 6 | u16 | Typed error, 0 through 18 |
| 8 | u32 | Client mutation nonce |
| 12 | u32 | Core operation ID |
| 16 | u32 | Requested deadline or pending-response retry interval, milliseconds |
| 20 | u32 | Exact body length |

States are Request=0, Complete=1, Pending=2, Failed=3, Cancelled=4.
Unknown values and inconsistent direction/state combinations are rejected.
Error numbering follows the explicit `Error` enum and its decoder in `src/lib.rs`.

Short requests and responses use zero nonce/operation ID/delay. A mutation
start uses a nonzero nonce, zero operation ID and a deadline from 1 to 10,000 ms.
Poll and Cancel use a nonzero nonce and operation ID, zero delay and an empty
body. Requests have error None and never carry the replay flag.

Complete has error None and zero delay. Pending has nonzero identities, error
None, an empty body and a retry interval from 1 to 10,000 ms. Failed has a
nonzero error other than Cancelled, an empty body and zero delay; pre-admission
failures may have a zero operation ID. Cancelled has error Cancelled, nonzero
identities, an empty body and zero delay. Short operations cannot be Pending
or Cancelled. The replay flag is valid only on a retained mutation response
with a nonzero operation ID, never on Poll or Cancel.

## Operation bodies

A path is a nonempty u8 byte length followed by at most 192 path bytes, without
NUL. Core validates traversal and its reserved persistence paths before upload
mutation. Sizes and offsets below are byte counts. All bodies must be consumed
exactly; successful responses are empty unless specified otherwise.

| ID | Operation | R2 request | Complete response |
| --- | --- | --- | --- |
| 0 | Capabilities | Empty | u32 supported mask, u32 max chunk, u32 max upload, u32 retention ms, u16 max path, u8 inflight capacity, u8 retained capacity |
| 1 | Stat | Path | u8 filesystem entry type, u32 size |
| 2 | List | Reserved | Unsupported |
| 3 | Read | Path, u32 offset, u16 count | Up to count raw bytes |
| 4 | UploadBegin | Nonzero u16 session, u32 expected size, path | Empty |
| 5 | UploadChunk | u16 session, u32 offset, u16 count, raw bytes | u32 accumulated size |
| 6 | UploadCommit | u16 session | Empty, after cooperative commit |
| 7 | UploadAbort | u16 session | Empty |
| 8 | Mkdir | Reserved | Unsupported |
| 9 | Delete | Reserved | Unsupported |
| 10 | Rename | Reserved | Unsupported |
| 11 | ConditionalReplace | Reserved | Unsupported |
| 12 | ConditionalDelete | Reserved | Unsupported |
| 13 | Poll | Empty | Retained operation result |
| 14 | Cancel | Empty | Cancelled, existing terminal result, or typed refusal |

IDs 6 and 8–12 are retained mutations. R2 advertises mask `0x60fb`, a
30,720-byte chunk limit, 524,288-byte upload limit and 30,000-ms result window.
One upload may be staged. R2 retains exactly **one commit per service lifetime**;
after admission, another UploadBegin is refused even after result expiration.
Normal repeated use and retained-record reclamation belong to R3.

Chunks must be sequential. A duplicate or incorrect offset is rejected without
appending. The R2 Manager does not automatically replay chunks; after a staging
failure it attempts Abort. Abandoned staging expires after 10,000 ms from Begin
on a foreground persistence turn, including when no further request arrives.
Playing postpones storage work and cleanup; short I/O is refused BusyPlaying.

The Core caller must open a persistence coordinator turn before `process` or
`advance`. Storage work runs in the foreground under existing quotas, leases,
commit journal and recovery rules. Cancel/deadline cleanup is allowed only
before the irreversible point. A cancellation after that point returns
CancelTooLate. Cleanup failure is reported and requires storage recovery.

## Loss, retry and identity

A repeated Commit with the same nonce, session and deadline returns the known
operation with `replayed=true`, without starting another commit. Changed
parameters for that nonce yield Conflict. Poll identifies the operation by
nonce and Core ID. After the terminal retention window it returns ResultExpired.
No result survives a Core restart; this is not an unbounded exactly-once claim.

A transport retry uses a **fresh request ID**, retaining the mutation nonce and
parameters. Reusing the timed-out request ID can route a reply to the previous
Bridge waiter. The R2 client retries one lost Commit/Poll exchange and rejects
a replay response on a fresh mutation attempt as a nonce collision. Exhausted
commit retries report an ambiguous outcome; they do not assert that nothing
was written. Reconnection, ID reuse over long sessions and reboot reconciliation
still require R3/R4 qualification.

## Reproduction

Run crate tests with `cargo test --manifest-path crates/filesystem-rpc/Cargo.toml`.
Core's `test/test_UnifiedFileSystemRpc/compare_codecs.py` compares both real
decoders and their byte-exact round trips on deterministic malformed/boundary
frames. See its command-line arguments for the two oracle executables.

In the full ms-dev-env workspace, build Core's `test_UnifiedFileTransfer`, set
`MS_CORE_RPC_PROBE` to that executable and run Bridge tests with
`cargo test --features unified-rpc-e2e`. This opt-in feature compiles the real
Manager source, uses the Bridge TCP control server/session and starts the native
Core service. Pipes and a logical clock replace serial hardware and wall time;
test duration is not a throughput or MCU CPU measurement.
