# Personal product relay

`talos-relay-server` is an optional transport library assembled by Talos Server.
It owns live connections and bounded HTTP delivery only. It has no product Store,
execution engine, scheduler, replica, or content log. Cloud-owned products use
Server Services directly and never pass through this library.

## Authentication boundary

A personal Host actively connects to `/talos/product/connect` over WSS. Its Hello
uses the existing current node lease and stable Host identity. Control-plane
registration, publication withdrawal, lease expiry, replacement, or shutdown
invalidate the associated connection. Relay does not create another registry.

The Server authenticates the product caller and requires the caller's account to
match the attached lease owner exactly. An administrator cannot use relay to
read another account's personal product. The existing typed Actor is projected
into the internal request envelope without credentials, authentication bypass,
or caller-controlled provenance. The Host checks the account against its current
lease owner before installing an in-process identity extension. The HTTP adapter
marks every product HTTP request as remote and applies the ordinary Service and
resource permissions. An external HTTP header or JSON field cannot install this
extension. Original Bearer and Cookie headers are not forwarded.

Talos Server is the trusted authentication delegate for this TLS-protected
transport. This is not an end-to-end encryption protocol. Optional Sync and its
original encryption/wire protocol are separate capabilities.

## Delivery and lifetime

- Requests use existing HTTP routes and typed business DTOs; the envelope adds
  transport identity, lane, stream sequence, and bounded flow-control credit.
- Each stream uses at most eight unconsumed 64 KiB data chunks. Connections admit
  at most 64 streams and have fixed bounded lane queues. Socket readers never
  await a slow stream's body queue. Control and interactive delivery can proceed
  while another stream waits for file consumption.
- Credit acknowledges transport consumption only. It does not acknowledge Sync
  replication, business persistence, Agent admission, or terminal settlement.
- Both sides probe the product socket every 15 seconds and require the matching
  Pong within 30 seconds. Writes also have a 30-second deadline. Node heartbeats
  cannot keep an unresponsive product socket alive.
- A changed lease or account closes the old transport. Renewing the same lease
  updates its deadline without interrupting existing requests. Host task abort,
  logout, retirement, and shutdown cancel all child delivery scopes.
- Dropping delivery cancels only that HTTP stream. Already admitted Agent turns
  retain the Worker execution lifetime and are observed through bounded cursor
  replay after reconnecting.
- HTTP Upgrade and provisional responses are rejected. Existing permissioned
  node media transports retain WebSocket desktop/browser delivery.

Unavailable connections fail explicitly; requests never fall back to a local
product or a synchronized copy. Protocol errors are confined to their stream
where possible; connection authentication and socket failures close its streams.

## Preview and source files

Personal preview URLs retain an isolated single-label wildcard origin. The
`r-{computer-base36}-{preview-base36}` label round-trips two UUIDs using standard
128-bit radix conversion and stays below the DNS label limit. Absolute root
asset paths therefore remain on the same routed origin. The Server restores the
original opaque preview UUID and forwards only the fixed read-only preview
resource endpoint. The Host's existing grant owns authorization, expiry, scope,
and close behavior; no Server preview mapping or content replica is created.

The optional source-file client uses authenticated product HTTP with
`x-talos-source-peer`. Resolution traverses current lease-authenticated personal
connections under the authenticated account. Missing aliases are unavailable;
multiple live matches in one account are a conflict. Resolution never starts
Sync. File metadata, content, bounded offset/limit JSON reads, update, and delete remain the original
FileService operations and resource permissions. Binary preview resource delivery uses standard HTTP Range; JSON content reads do not apply byte-range headers to a base64 representation.
