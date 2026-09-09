# Filesystem wire contract v4 — R3d conditional mutations

This unpublished, dependency-free crate owns only the borrowed wire envelope.
Core implements the same contract in `UnifiedFileSystemRpc.{hpp,cpp}`. It does
not translate requests into the previous named filesystem/job frames.

The application and Manager UI still use the previous protocol. This isolated
prototype is not a released compatibility promise. Operation bodies beyond the
implemented subset below are reserved for the coordinated migration.

## Envelope

All integers are little-endian. A frame is exactly 24 bytes plus its body; no
padding or trailing bytes are accepted. Maximum body length is 32,512 bytes.

| Offset | Type | Meaning |
| --- | --- | --- |
| 0 | u8 | `0xfc` request, `0xfd` response |
| 1 | u8 | Version, exactly `4` |
| 2 | u8 | Operation, 0 through 14 |
| 3 | u8 | State in bits 0–6; bit 7 marks a retained mutation replay |
| 4 | u16 | Nonzero transport request ID, echoed in the response |
| 6 | u16 | Typed error, 0 through 20 |
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
nonzero error other than Cancelled and zero delay; its body is empty except for
the conditional result described below. Pre-admission
failures may have a zero operation ID. Cancelled has error Cancelled, nonzero
identities, an empty body and zero delay. Short operations cannot be Pending
or Cancelled. The replay flag is valid only on a retained mutation response
with a nonzero operation ID, never on Poll or Cancel.

## Operation bodies

A path is a nonempty u8 byte length followed by at most 192 path bytes, without
NUL. Core validates traversal and its reserved persistence paths before upload
mutation. Sizes and offsets below are byte counts. All bodies must be consumed
exactly; successful responses are empty unless specified otherwise.

| ID | Operation | Request | Complete response |
| --- | --- | --- | --- |
| 0 | Capabilities | Empty | u32 supported mask, u32 max chunk, u32 max upload, u32 retention ms, u16 max path, u8 inflight capacity, u8 retained capacity |
| 1 | Stat | Path | u8 filesystem entry type, u32 size |
| 2 | List | Path, u16 start index, u8 limit (1–8), u32 snapshot ID | u32 snapshot ID, u16 start index, u8 count, u8 has more (0/1), then entries |
| 3 | Read | Path, u32 offset, u16 count | Up to count raw bytes |
| 4 | UploadBegin | u32 expected size, path | Nonzero u32 upload identity, assigned by Core |
| 5 | UploadChunk | u32 upload identity, u32 offset, u16 count, raw bytes | u32 accumulated size |
| 6 | UploadCommit | u32 upload identity | Empty, after cooperative commit |
| 7 | UploadAbort | u32 upload identity | Empty |
| 8 | Mkdir | Path | Empty, retained |
| 9 | Delete | Path, u8 recursive (0/1) | Empty, retained |
| 10 | Rename | Source path, destination path | Empty, retained |
| 11 | ConditionalReplace | 32-byte expected SHA-256, 32-byte replacement SHA-256, current path, staging path | Conditional result |
| 12 | ConditionalDelete | 32-byte expected SHA-256, current path | Conditional result |
| 13 | Poll | Empty | Retained operation result |
| 14 | Cancel | Empty | Cancelled, existing terminal result, or typed refusal |

IDs 6 and 8–12 are retained mutations. R3d advertises mask `0x7fff`, a
30,720-byte chunk limit, 524,288-byte upload limit and 30,000-ms result window.
One upload may be staged. A fixed registry retains **32 operation results**.
A terminal record expires at 30,000 ms; a pending operation is never reaped by
this timer. Saturation refuses Begin before opening a write session and never
evicts an unexpired result. Reclaimed slots allow further uploads.

Each List entry is a u8 name length, name bytes (1–63 bytes), u8 type
(Missing=0, File=1, Directory=2, Other=3), u32 size and u8 name-truncated
flag (0/1). A first page uses start=0 and snapshot=0. The response supplies
the shared catalog's nonzero snapshot ID; subsequent pages must echo it.
An unavailable/replaced snapshot or changed storage identity returns Conflict,
requiring a fresh listing. The catalog increments its ID before every scan,
including a failed scan, and refuses to wrap at u32 exhaustion. IDs are scoped
to the catalog lifetime, not reboot. A snapshot contains at most 256 entries;
overflow is an error, not a silently truncated directory. Pagination reads the
existing shared snapshot with no second catalog allocation or per-page scan.
An intervening use of another directory may invalidate continuation.

Manager validates page identity, index, count, progress, types, flags, names and
exact body consumption. It returns the complete bounded list or an error; it
does not return partial pages on a conflict. Its read-batch API preserves the
existing maximum of eight simultaneous reads (245,760 requested bytes), checks
offset overflow and every exact response length, and returns buffers in request
order. The caller must stream successive batches and handle file changes; this
API does not promise a multi-request read snapshot or a firmware throughput gain.

Version 4 rejects the unpublished version 2/3 prototypes. Version 3 introduced
Core-assigned upload identities; version 4 permits bounded conditional details
on failed results and adds StorageFailure=19 and TooLarge=20. Negotiate
capabilities before mutation; do not silently downgrade.
The upload identity is the coordinator's nonzero, non-wrapping job ID. It is
also the commit's operation ID and cannot address a later upload during that
coordinator's lifetime, including after result reclamation. A second nonce
cannot replace an already pending commit.

Mkdir, Rename and nonrecursive Delete execute under a mutation lease and retain
their result in the same registry as commits. A recursive Delete uses the existing
hide-first, bounded tree-cleanup continuation. Cancel and deadline may stop it
before the canonical tree is hidden; afterward CancelTooLate is returned and
cleanup continues. A cleanup failure leaves the recovery marker and blocks new
mutations until the existing recovery plan completes. Upload and tree continuations
share one variant storage area and cannot be active together.

Retained requests compare operation kind, deadline and a SHA-256 fingerprint of
the complete request body (bounded to 450 bytes for the implemented mutations).
This uses collision-resistant equality, not a short checksum. Different parameters
or an operation-kind collision under the same nonce return Conflict. The registry
does not preserve nonce tombstones beyond the retention window: clients must not
blindly resubmit a path mutation after expiry or reboot and must reconcile an
ambiguous outcome. Upload identities additionally prevent a stale upload request
from addressing a later session during the coordinator lifetime.

Conditional mutations reuse the existing conditional transaction/commit plan and
journal. Staging must be a nonreserved file under `tmp/`; paths are normalized,
FAT short-name aliases and identical source/staging paths are rejected. The
journal's operation ID is Core's assigned job ID. All three continuations share
one variant storage area. Cancel/deadline may stop before the journal's irreversible
point; afterward cleanup/recovery owns the durable outcome.

A terminal conditional result has exactly 35 bytes: u8 outcome (None=0,
Applied=1, AlreadyApplied=2), u8 subject (None=0, Source=1, Staging=2), u8
has-observed-hash (0/1), and 32 SHA-256 bytes. If the flag is zero all hash bytes
must be zero. The outcome stays in the body and the error stays in the envelope;
there is no second legacy status field. Complete returns this body with error
None. Failed may carry it only for ConditionalReplace, ConditionalDelete, Poll
or Cancel with a nonzero operation ID. Pre-admission rejections and Cancelled
have empty bodies. Replays and queries preserve terminal details, including the
observed digest on a precondition failure. StorageFailure preserves the plan's
generic storage status without inventing whether the failed primitive was a read
or write. Failed does not assert that no file changed: durable recovery may remain
necessary. Reconcile the inner ordinary promotion journal before the conditional
journal, as ProductStorageRecoveryPlan already does.

Chunks must be sequential. A duplicate or incorrect offset is rejected without
appending. The Manager does not automatically replay chunks; after a staging
failure with a known upload identity it attempts Abort. If Begin's response is
lost, the client does not guess an identity to abort. Core expires the abandoned
staging after 10,000 ms from Begin on a foreground persistence turn, including
when no further request arrives. A deferred autosave reaching the existing
2,000-ms deferral limit also releases an idle staging upload.
Playing postpones storage work and cleanup; short I/O is refused BusyPlaying.

The Core caller must open a persistence coordinator turn before `process` or
`advance`. Storage work runs in the foreground under existing quotas, leases,
commit journal and recovery rules. Cancel/deadline cleanup is allowed only
before the irreversible point. A cancellation after that point returns
CancelTooLate. Cleanup failure is reported and requires storage recovery.
A short append or failed cooperative step schedules cleanup on a separate
measured promotion turn, so it cannot spend cleanup I/O inside a read/chunk quota.
Retained results carry the media generation; a query against another generation
returns MediaChanged. This does not prove recovery after power loss.

## Loss, retry and identity

A repeated Commit with the same nonce, session and deadline returns the known
operation with `replayed=true`, without starting another commit. Changed
parameters for that nonce yield Conflict. Poll identifies the operation by
nonce and Core ID. After the terminal retention window it returns ResultExpired.
No result survives a Core restart; identity uniqueness here is scoped to the coordinator lifetime, not across reboot. This is not an unbounded exactly-once claim.

A transport retry uses a **fresh request ID**, retaining the mutation nonce and
parameters. Reusing the timed-out request ID can route a reply to the previous
Bridge waiter. The client retries one lost Commit/Poll exchange and rejects
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
Core service. Pipes and a logical clock replace serial hardware and wall time.
The native server performs eight admitted foreground turns before each exchange;
it does not make commit progress depend on one step per Manager poll. This
bounded schedule is not a firmware timing model, and test duration is not a
throughput or MCU CPU measurement.
