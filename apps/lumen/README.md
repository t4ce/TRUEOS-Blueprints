# Lumen

This is the first replicatable Lumen template Blueprint.

The Blueprint owns the fixed `time()` and `move(x,y)` tool schemas, conversation
turn and reply tail, strict tool-call adapter, and a portable checkpoint containing
short-convolution and attention KV state. TRUEOS retains the immutable LFM2.5
model, tokenizer artifact, packed C++/IGC program, and GuC/RCS execution lane.

On initial launch it prefills the pinned model's native JSON tool-list entries
once. A user turn may finish directly, call read-only `time()`, or call
`move(x,y)` with finite centre-relative coordinates in `-0.5..=0.5`. `time()`
receives a bounded UTC tool result as an actual `tool` role message and one
final continuation. A valid `move()` is dispatched to Spirit at `(x + 0.5,
y + 0.5)` and is terminal: it has no tool result, continuation decode, or
textual acknowledgement.

The current LFM decoder exposes argmax only. It cannot mask a branching tool
grammar while preserving model-selected numeric arguments, so tool-call mode,
tool name, and move coordinates are model-authored then strictly parsed and
validated by the Blueprint. The required Liquid chat framing remains intact. At
`PreparePause` it asks the kernel Lumen capability to export mutable inference
state into Blueprint memory, releases the live inference session, and reports
`Ready`. After same-instance or clone `Resume`, it uploads that state into a
fresh kernel session and reacquires execution capability.

The intended bring-up is:

1. Start `lumen.bp`; wait for `template ready`.
2. Replicate the paused template through the F2 lifecycle.
3. Resume the private child and type a prompt.
4. Confirm the first user turn starts at the restored prefix position without
   re-prefilling the time-tool schema.

Typing a prompt before replication remains supported for ABI bring-up.
