# Guest runtime evidence, 2026-10-14

The baseline and follow-up runs both reported **PASS** with completion mask `0x7f`: all seven scenarios ended successfully, and all 14 selected real dependency entries were captured once. All 14 physical chains reached recognized roots, all entry stack pointers had `RSP % 16 == 8`, and all requested raw stack intervals had zero unreadable bytes. Both owned experiment QEMU processes were stopped after capture.

| Native filesystem parking checkpoint | Baseline `run-final` | Follow-up `run-stack-fold` |
| --- | ---: | ---: |
| Physical frames at `trueos_thread_context_swap` entry | 19 | 19 |
| Captured entry RSP to recorded stack end | 166,824 bytes | 3,048 bytes |
| Largest adjacent nonzero RBP gap | 163,936 bytes | 480 bytes |
| Shared `trueos_kernel_sync_fs_read_file` RBP gap | 163,936 bytes | 160 bytes |
| Unreadable raw stack bytes | 0 | 0 |

The change removes 163,776 bytes from the captured native interval. The baseline had inlined a guest-only 160 KiB buffer into the shared synchronous read ABI. Adding `#[inline(never)]` to the kernel's `guest_fs_read_file` confines that storage to the guest branch. It changes neither the probe source nor the Blueprint payload or capture plan. The frame-pointer gaps include the saved frame record; they are not exact compiler local-variable sizes. This experiment measures a particular active native stack at a particular first hit, not a universal stack maximum.

The kernel-image symbol map and runtime context have separate meanings. Launch ownership was RAM root 2, VM 0. Eleven captures used versioned VMX L2 Hull snapshots and ended at `trueos_vmx_guest_entry`. Three worker captures used the active native stack of logical thread 1 on carrier/AP slot 2 and ended at `trueos_thread_context_start`; worker stack containment was checked. A kernel-owned symbol can appear in an L2 snapshot, so symbol ownership alone does not identify the execution context.

Capture boundaries are exact function entries before their prologues. The immediate return address is read at `[RSP]`, followed by bounded frame-pointer records. Capture records retain register evidence, raw readable ranges, symbol mapping, inline DWARF attribution and explicit unwind termination. First-hit coverage means the first accepted hit at each selected target during its begin/end interval; unrelated kernel hits without probe ancestry were ignored. Passing these fixture paths and recognizing their physical roots does not establish an exhaustive runtime trace or reconstruct previous/future asynchronous turns.

Inputs are pinned in each run directory. The common Blueprint SHA-256 is `0e7e445be67c7338e92f5e315a651fd873ee01b6a7276e0f9425db7693a4c7ce`; the common plan SHA-256 is `6090ba500edac982604490cadb0093a0106f32eef65d8f690025e2e8792f9d90`.

| Kernel | SHA-256 |
| --- | --- |
| Baseline | `9b8d6d7f6a2d5053b8d2999fc77619fb119359aefbe24209991991c967127ed4` |
| Follow-up | `251f223677ce7e7f1a60aa40a587872d44b1acaa926157fbe2cfae5a126a57da` |

Evidence in the sibling TRUEOS repository:

- [Experiment notes](../../../../TRUEOS/tools/docs/depthchecker-factchecker-2026-10-14.md)
- [Baseline overview](../../../../TRUEOS/bld/depthchecker_factchecker/report-final/overview.md) and [raw result](../../../../TRUEOS/bld/depthchecker_factchecker/run-final/result.json)
- [Follow-up overview](../../../../TRUEOS/bld/depthchecker_factchecker/report-stack-fold/overview.md) and [raw result](../../../../TRUEOS/bld/depthchecker_factchecker/run-stack-fold/result.json)
- [Baseline native parking capture](../../../../TRUEOS/bld/depthchecker_factchecker/run-final/captures/native_thread-0f32306e5720/capture.json)
- [Follow-up native parking capture](../../../../TRUEOS/bld/depthchecker_factchecker/run-stack-fold/captures/native_thread-0f32306e5720/capture.json)
