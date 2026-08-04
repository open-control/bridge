# Persistence job protocol v1

Status: normative additive supplier for midi-studio `L-R05-05`.

This document defines the byte-exact asynchronous persistence-job contract
between ms-manager, oc-bridge and midi-studio Core. It does not make the Bridge
the owner of controller work: Core owns jobs and durable state; Bridge relays
matched responses and quarantines unmatched job responses; Manager owns client
nonces and polling.

## Discovery and compatibility

The existing Core filesystem capabilities response uses feature bit
`1 << 4` to advertise this protocol. Bridge protocol supplier version 1 and
that Core bit are both required before a client selects jobs.

| Core bit 4 | Bridge supplier | Client mode |
| --- | --- | --- |
| absent | any | bounded stopped-only legacy filesystem RPC |
| present | absent/unknown | bounded stopped-only legacy filesystem RPC |
| present | v1 or later | persistence jobs v1 |

Existing filesystem schema 1 and local `OCRQ/OCRS` version 1 bytes do not
change. Old Core omits bit 4. Old Manager never sends the reserved IDs.

## Open-control envelope

The job protocol reserves the next free filesystem pair:

| Direction | Message ID | Canonical name | Schema |
| --- | ---: | --- | ---: |
| request | `0xFC` | `FsJobRequest` | 1 |
| response | `0xFD` | `FsJobResponse` | 1 |

Every message starts with the existing variable-length open-control envelope:

```text
u8 messageId
u8 nameLength
u8 name[nameLength]
u8 schema
u16 requestId
```

Integers are little-endian. `requestId` is the existing per-frame correlation
value; it is not a job identity and may wrap according to the legacy transport.

## Request

The application header after the envelope is exactly 16 bytes.

| Offset | Type | Field | Rule |
| ---: | --- | --- | --- |
| 0 | `u8` | command | values below |
| 1 | `u8` | flags | zero in v1 |
| 2 | `u16` | reserved | zero |
| 4 | `u32` | client nonce | command-dependent |
| 8 | `u32` | job ID | command-dependent |
| 12 | `u32` | total deadline ms | command-dependent |

### Commands

| Value | Command | Identity/deadline | Body |
| ---: | --- | --- | --- |
| 0 | `CAPABILITIES` | nonce/job/deadline all zero | empty |
| 1 | `START` | nonce non-zero, job zero, deadline `1..10000` | one complete legacy schema-1 filesystem request, at most 32,512 B |
| 2 | `POLL` | nonce and job non-zero, deadline zero | empty |
| 3 | `CANCEL` | nonce and job non-zero, deadline zero | empty |

The inner start frame must use an existing request ID from `0xE0..0xFA`; a job
frame cannot recursively contain `0xFC/0xFD`. Unknown commands, non-zero flags
or reserved fields, invalid identity combinations, invalid deadlines and
malformed/oversized bodies are rejected.

## Response

The application header after the envelope is exactly 20 bytes.

| Offset | Type | Field | Rule |
| ---: | --- | --- | --- |
| 0 | `u8` | command | echoed request command |
| 1 | `u8` | state | values below |
| 2 | `u8` | typed error | values below |
| 3 | `u8` | flags | known mask only |
| 4 | `u32` | client nonce | exact echo, except capabilities uses zero |
| 8 | `u32` | job ID | stable Core identity, except an unadmitted start may use zero |
| 12 | `u32` | retry-after ms | pending states only, at most 10,000 |
| 16 | `u32` | progress per mille | `0..1000` |

`COMPLETED` appends the exact terminal legacy response frame, at most 32,512 B.
Other job states append no body. `CAPABILITIES` has its own fixed body below.

### States

| Value | State | Terminal |
| ---: | --- | --- |
| 0 | `NONE` | capabilities only |
| 1 | `ACCEPTED` | no |
| 2 | `PENDING` | no |
| 3 | `COMPLETED` | yes |
| 4 | `CANCEL_PENDING` | no |
| 5 | `CANCELLED` | yes |
| 6 | `FAILED` | yes |
| 7 | `REJECTED` | yes |

Accepted, pending, cancel-pending and completed use error `NONE`. Cancelled uses
`CANCELLED`. Failed and rejected require a non-zero typed error.

### Typed errors

| Value | Error |
| ---: | --- |
| 0 | `NONE` |
| 1 | `INVALID_MESSAGE` |
| 2 | `INVALID_ARGUMENT` |
| 3 | `UNSUPPORTED` |
| 4 | `NOT_FOUND` |
| 5 | `BUSY_PLAYING` |
| 6 | `RESOURCE_EXHAUSTED` |
| 7 | `CONFLICT` |
| 8 | `PRECONDITION_FAILED` |
| 9 | `DEADLINE_EXCEEDED` |
| 10 | `MEDIA_CHANGED` |
| 11 | `STORAGE_UNAVAILABLE` |
| 12 | `STORAGE_READ_FAILED` |
| 13 | `STORAGE_WRITE_FAILED` |
| 14 | `STORAGE_CORRUPT` |
| 15 | `CANCELLED` |
| 16 | `INTERNAL` |
| 17 | `LEGACY_BUSY` |
| 18 | `LEGACY_STORAGE_ERROR` |

The last two values explicitly preserve uncertainty from old schema-1 results;
they must not be presented as a more precise cause.

### Response flags

| Bit | Flag | Valid use |
| ---: | --- | --- |
| 0 | `DUPLICATE_START` | start returned the existing job |
| 1 | `LEGACY_MAPPED` | typed or legacy projection lost information |
| 2 | `TERMINAL_RETAINED` | poll returned a retained terminal result |
| 3 | `CANCEL_TOO_LATE` | cancel crossed an irreversible durable boundary |

Bits 4–7 are zero in v1. A flag on the wrong command/state is invalid.

## Capabilities body

`CAPABILITIES` responds with state/error/flags and all identity/progress fields
zero, followed by exactly 24 bytes.

| Offset | Type | Field | v1 value |
| ---: | --- | --- | ---: |
| 0 | `u8` | protocol version | 1 |
| 1 | `u8` | max concurrent jobs | 2 |
| 2 | `u16` | reserved | 0 |
| 4 | `u32` | feature flags | `0x3F` |
| 8 | `u32` | max inner request bytes | 32,512 |
| 12 | `u32` | max inner response bytes | 32,512 |
| 16 | `u32` | max total deadline ms | 10,000 |
| 20 | `u32` | terminal retention ms | 30,000 |

Feature bits are start, poll, cancel, terminal retention, typed errors and
legacy mapping in bits 0–5 respectively.

## Identity and lifecycle

Manager allocates non-zero `u32` client nonces and fails closed before reuse.
Core allocates non-zero `u32` job IDs and fails closed before rollover. The
tuple `{clientNonce, jobId}` is required for poll and cancel; either mismatch is
`NOT_FOUND`.

For a retained nonce:

- byte-identical `START` returns the same job ID and current state with
  `DUPLICATE_START`; it never starts work twice;
- a different inner request returns `REJECTED/CONFLICT` and the original job
  ID; the original operation is unchanged.

Core retains terminal state and the byte-exact inner terminal response for
30,000 ms. Poll at the exact retention boundary succeeds and sets
`TERMINAL_RETAINED`; the next millisecond returns `NOT_FOUND`. Elapsed-time
comparison is rollover-safe.

Cancel at a safe pre-publication checkpoint transitions through
`CANCEL_PENDING` to `CANCELLED`. Cancel after an irreversible rename/flush or
other declared durable boundary sets `CANCEL_TOO_LATE`; the job continues and
its terminal result remains pollable. An atomic storage primitive is never
interrupted mid-call.

## Late-response quarantine

Bridge first attempts exact pending-RPC matching. A matched `0xFD` is returned
to its local waiter. Any unmatched `0xFD`, including one arriving after waiter
expiry, is consumed and never forwarded to Bitwig. Ordinary unmatched
controller messages keep the existing controller-to-host path.

## Schema-1 compatibility mapping

Legacy-to-typed mapping is total:

| Legacy status | Typed error | Lossy |
| --- | --- | --- |
| `OK` | `NONE` | no |
| `INVALID_MESSAGE` | `INVALID_MESSAGE` | no |
| `INVALID_ARGUMENT` | `INVALID_ARGUMENT` | no |
| `NOT_FOUND` | `NOT_FOUND` | no |
| `BUSY` | `LEGACY_BUSY` | yes |
| `TOO_LARGE` | `RESOURCE_EXHAUSTED` | yes |
| `STORAGE_ERROR` | `LEGACY_STORAGE_ERROR` | yes |
| `INVALID_STATE` | `INTERNAL` | yes |
| `UNSUPPORTED` | `UNSUPPORTED` | no |
| `PRECONDITION_FAILED` | `PRECONDITION_FAILED` | no |

Typed-to-legacy projection preserves the six exact pairs above. Busy maps to
`BUSY`; resource exhaustion to `TOO_LARGE`; all media/storage causes to
`STORAGE_ERROR`; conflict, deadline, cancellation and internal failure to
`INVALID_STATE`. Every such collapsed projection is marked `LEGACY_MAPPED`.

This compatibility is leased only through `L-R05-09B`. Core removes its old
consumer path first in `L-R05-09C`; Bridge removes these mappings only after the
compatible Manager/Core graph has zero old consumer.

## Resource contract

The codec and policy module performs no heap allocation and retains no job,
payload, task, channel, mutex or collection. Encoders write into caller buffers;
decoders borrow input slices. Bridge raw pending-controller capacity remains
eight. Core payload placement remains governed by its PSRAM/RAM contracts.
