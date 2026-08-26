# Lumen

This is the first replicatable Lumen template Blueprint.

The Blueprint owns the fixed `time()` tool schema, conversation turn and reply
tail, strict tool-call adapter, and a portable checkpoint containing
short-convolution and attention KV state. TRUEOS retains the immutable LFM2.5
model, tokenizer artifact, packed C++/IGC program, and GuC/RCS execution lane.

On initial launch it prefills the pinned model's native JSON tool-list entry for
its fixed read-only `time()` schema once. A user turn may finish directly or
select one no-argument `time()` call. After the model selects the native
tool-call start token, the inference capability constrains the remaining tokens
to that exact call; its bounded UTC
tool result is appended as an actual `tool` role message and followed by one
final continuation. The required Liquid chat framing remains intact. At
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
