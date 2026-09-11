> Implementation update: the current server runs the image slideshow described in [README.md](README.md). The world-catalog design below is retained as historical planning; world requests are no longer served.

# cubesrv design

`cubesrv` is a small, authoritative, memory-only multiplayer service for
`Cubes`. It deliberately owns no game rules beyond presence, loading the
authored world/catalog data, and accepting additive asset placements.

The design has two network planes in one Blueprint process:

- Axum/TCP is the control and mirror-content plane. It serves health, the
  catalog, and the original `.cubes` blobs byte-for-byte for inspection and
  conventional caching.
- UDP is the live plane. It carries player poses, placement intents, placement
  results, bounded world-state snapshots, and a chunked form of the immutable
  catalog blobs.

Blob traffic is separately paced and always yields to live traffic. A client
can perform the whole first integration over one UDP port, while Axum remains
useful for diagnostics and tooling. Once cached by digest, a client uses only
small live datagrams after joining.

## Initial scope

- 27 authored worlds from `Cubes/Cube/lvl27`.
- 49 placeable assets from `Cubes/Cube/Assets`.
- Player position and orientation vectors in one selected world.
- Server-authoritative, additive asset placement.
- Memory-only placed state. Restarting `cubesrv` resets all placements and
  produces a new server epoch.
- No accounts, persistence, removal, damage, inventory, ownership, or deeper
  game logic.

The server embeds the source `.cubes` files during the Blueprint build. It
does not decode and re-encode immutable world files for transfer. The existing
strict-grid nature v1 format remains the content contract.

## Coordinate contract

Protocol coordinates use the logical coordinate system consumed by
`CubesWalkerCam`, before renderer scaling:

- player `position` is `[f32; 3]`;
- player `orientation` is a normalized forward vector `[f32; 3]`;
- authored +Y is converted exactly once by the client, as it is today;
- placement anchors are signed quarter-cell coordinates `[i16; 3]`;
- a placement face is one of `-X, +X, -Y, +Y, -Z, +Z`;
- world IDs and asset IDs are indices into the digest-stamped catalog, not
  filenames sent repeatedly in live packets.

Quarter-cell integer anchors avoid float disagreement in placement. The
server uses the same asset transform as `asset_brush::place` and broadcasts
the resulting canonical cubes. Clients do not get to submit arbitrary cube
colors, sizes, or geometry.

## HTTP API

The initial Axum router exposes:

| Route | Result |
| --- | --- |
| `GET /healthz` | epoch, uptime, UDP port, player count, placed cube count |
| `GET /v1/catalog` | protocol version plus world and asset IDs, names, lengths, and SHA-256 digests |
| `GET /v1/worlds/{id}` | original world `.cubes` bytes |
| `GET /v1/assets/{id}` | original asset `.cubes` bytes |
| `GET /v1/state` | small diagnostic summary; never the realtime transport |

Blob responses include an ETag derived from SHA-256 and support normal HTTP
cache validation. The catalog also contains the UDP protocol version and the
current server epoch. A changed epoch invalidates only dynamic state; a changed
blob digest invalidates that cached blob.

## UDP envelope

All integers are little-endian and all floats are finite IEEE-754 `f32`.
Datagrams stay at or below 1200 bytes to avoid IP fragmentation.

Every datagram begins with this fixed header:

| Field | Type | Meaning |
| --- | --- | --- |
| magic | `[u8; 4]` | `CBS1` |
| version | `u8` | live protocol version, initially `1` |
| kind | `u8` | message kind |
| flags | `u16` | initially zero |
| epoch | `u32` | server epoch; zero in the first client `Hello` |
| peer_id | `u32` | server-issued ID; zero before `Welcome` |
| sequence | `u32` | sender-local sequence for this message stream |

Unknown versions, kinds, flag bits, non-finite values, bad lengths, and stale
epochs are ignored. Sequence comparison uses wrapping serial-number arithmetic,
not ordinary integer comparison.

### Messages

| Kind | Direction | Purpose |
| --- | --- | --- |
| `Hello` | client to server | client nonce and supported protocol version |
| `Welcome` | server to client | epoch, peer ID, catalog digest, tick rates |
| `Pose` | client to server | world ID, position, orientation |
| `PoseFrame` | server to client | latest poses of peers relevant to the receiver |
| `PlaceAsset` | client to server | request ID, world ID, asset ID, quarter-cell anchor, face |
| `PlaceResult` | server to client | accepted/rejected status and authoritative placement ID |
| `RegionWant` | client to server | region key and newest fully installed generation |
| `RegionPart` | server to client | one fragment of a complete region snapshot |
| `RegionAck` | client to server | newest fully installed region generation |
| `CatalogWant` / `Catalog` | client to server / server to client | digest-stamped world and asset manifest |
| `BlobWant` | client to server | blob kind, ID, and missing chunk window |
| `BlobPart` | server to client | one digest-bound chunk of an original `.cubes` blob |
| `Ping` / `Pong` | both | liveness and RTT measurement |

The source address plus `(epoch, peer_id, client_nonce)` identifies a live
client. A new valid `Hello` may rebind a peer after its UDP source port changes.
This is session identification and accidental-spoof resistance, not security;
authentication can be added later without changing region state semantics.

## Player flow

Clients normally send `Pose` at 30 Hz. The server retains only the newest valid
sequence from each peer and emits `PoseFrame` at 20 Hz. A slow client therefore
does not create an unbounded queue: old poses are replaced, not delivered late.

Each pose includes its world ID. Initially the server sends a client only peers
in that same world. A client interpolates between received frames and may
extrapolate briefly; transport loss never stalls rendering. A peer expires
after five seconds without a valid packet.

Orientation is a forward vector for the first protocol. The server rejects a
near-zero vector and normalizes reasonable vectors before rebroadcast. A later
version can add a quaternion if roll becomes meaningful.

## UDP catalog and world streaming

`Catalog` provides the same ordered IDs, lengths, and SHA-256 digests as the
HTTP catalog. It may be fragmented when necessary. The catalog itself has a
SHA-256 digest included in `Welcome`, so clients can retain it across sessions.

`BlobPart` carries `(blob kind, blob ID, blob digest prefix, chunk_index,
chunk_count, byte_offset, bytes)`. Chunks use at most roughly 1 KiB of payload
so the complete datagram remains below 1200 bytes. The receiver requests a
window of missing chunks with `BlobWant`; a compact bit mask allows selective
retry rather than restarting a file. The client publishes a blob to its cache
only after checking its total length and full SHA-256 digest.

The server uses bounded per-peer pacing and does not retain a copy of sent
chunks. All chunks are slices of embedded immutable bytes and can be recreated
from a request. Pose frames, placement results, and dirty region parts have
priority over blob parts, so loading a world cannot make player movement
stutter. A per-peer blob rate cap and a global response budget prevent one
client from turning UDP requests into an amplification or memory problem.

HTTP downloads are an equivalent optional path, not a prerequisite for a
`Cubes` client.

## World synchronization

Dynamic world state is divided into logical 4×4×4 world-cell regions. Because
placement uses quarter cells, one region spans 16×16×16 fine occupancy cells.
A region key is `(world_id, region_x, region_y, region_z)` with signed region
coordinates.

Each region has a monotonically increasing `u64 generation`. Its authoritative
snapshot is the set of placed cube records whose minimum occupied fine cell is
inside that region. A cube receives a globally unique `u64 cube_id`, so a cube
touching another region is still represented once and clients can de-duplicate
or index it consistently.

A canonical placed cube record contains:

- `cube_id: u64`;
- minimum quarter-cell `[i16; 3]`;
- size in quarter cells `u8`;
- RGB555 color `u16`;
- semantic part ID `u8`;
- placement ID `u64`.

`RegionPart` fragments a snapshot into independently sized datagrams and
contains `(region key, generation, part_index, part_count)`. A client installs
a generation only after all of its parts arrive. Receiving any newer generation
discards an incomplete older assembly. Thus, if the same region changes faster
than the network can send every intermediate update, obsolete generations are
skipped and the newest complete snapshot wins.

Convergence uses three layers:

1. An accepted placement immediately marks every touched region dirty.
2. Dirty region snapshots are sent promptly and repeated a small bounded number
   of times until acknowledged or superseded.
3. Clients rotate `RegionWant` requests across nearby regions as low-rate
   anti-entropy. The server answers only when its generation is newer.

There is no unbounded per-client change queue. Snapshot work is coalesced by
`(region, generation)`, and a newer dirty generation replaces unsent older
work. This is the important behavior for a rapidly changing location.

Although removal is out of scope, snapshots are complete replacement sets,
not append-only deltas. That means later removal or correction can be added
without redesigning synchronization.

## Authoritative placement

On click, the client keeps its existing white preview as a pending local marker
and sends `PlaceAsset` with a fresh request ID. The server:

1. de-duplicates `(peer_id, request_id)` so retries are safe;
2. validates world, asset, anchor, face, bounds, portal clearance, and the
   maximum placed-cube budget;
3. expands the catalog asset with the shared deterministic placement transform;
4. rejects the entire placement if any resulting occupied fine cell overlaps
   authored terrain or an earlier placement;
5. commits all cubes atomically, advances touched region generations, and
   returns `PlaceResult::Accepted`;
6. broadcasts current region snapshots.

The client removes the white pending marker when it receives a rejection. On
acceptance it may retain the marker until a complete authoritative region
generation containing the placement ID is installed. The real colored cubes
then replace the marker and naturally enter the existing hull-shading/reveal
path.

This makes the visual handoff insensitive to packet order: `PlaceResult` may
arrive before or after `RegionPart` without briefly losing the placement.

## In-memory model

The server needs only these bounded structures:

- immutable embedded world and asset catalogs;
- decoded authored occupancy for 27 worlds;
- a bounded peer table;
- placed cubes indexed by world and cube ID;
- fine-cell occupancy for collision checks;
- region generation and membership maps;
- a small recent-request result cache per peer;
- bounded dirty-region send state per peer.

All dynamic structures have explicit caps. The first defaults should be 64
peers, 16,384 placed cubes per world, 256 remembered request results per peer,
and 1200-byte maximum datagrams. Exceeding a cap returns a placement error or
drops the least-recently-seen peer; it never grows memory without bound.

## Runtime and Blueprint lifecycle

The process uses a current-thread TRUEOS network runtime and a `LocalSet`, like
the existing Axum Blueprint servers. Axum uses the lifecycle-aware TCP listener.
The UDP task owns no logical state; if its socket lease is revoked by
pause/resume, it drops and rebinds the socket while shared in-memory state stays
alive. The advertised UDP port is updated after every bind.

`cubesrv` should be marked `replicatable = true`. A clone is a separate,
memory-only server epoch and is not a replicated multiplayer room. Replicating
live room state is explicitly outside the initial contract.

## Source layout

The implementation should remain small and keep transport parsing testable:

```text
apps/cubesrv/
  Cargo.toml
  build.rs             embed and validate Cubes catalogs
  server.rs            runtime, Axum router, task startup
  protocol.rs          allocation-free UDP encode/decode
  catalog.rs           .cubes validation and immutable metadata
  state.rs             peers, placement, occupancy, region generations
  sync.rs              pose frames and region snapshot scheduling
  DESIGN.md
```

The build must fail when the expected 27 worlds or 49 assets are missing or a
blob is not valid strict-grid nature v1. Catalog order is lexical and therefore
stable, but clients still bind cached IDs to the catalog digest.

## Integration order

1. Build the app, catalog, Axum endpoints, UDP `Hello`/`Welcome`, UDP catalog
   and blob chunks, and tests for malformed datagrams.
2. Let Cubes fetch/cache the catalog and default world blobs entirely over UDP,
   then join and render remote poses.
3. Add `PlaceAsset`, pending white markers, authoritative expansion, and
   collision validation shared with Cubes.
4. Add fragmented region snapshots, acknowledgements, and anti-entropy.
5. Stress one hot region with loss, duplication, reordering, and more mutations
   per second than the sender can transmit; assert eventual equality with the
   server after mutation stops.

The compatibility rule is simple: immutable content is addressed by digest,
live packet layouts are versioned, requests are idempotent, and dynamic state
converges only through complete generation-stamped snapshots.
