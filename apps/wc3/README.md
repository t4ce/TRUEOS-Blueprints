# Warcraft III Blueprint

This Blueprint owns the Windows XP compatibility semantics used by the
Warcraft III 1.00 launcher. It loads the original PE32 image, patches imports
to x86 `VMCALL` thunks, and services those traps in ordinary Rust userspace.

The only privileged boundary is `trueos::x86`: generic address-space,
context, memory and trap operations. The original kernel launcher remains a
temporary parity oracle and is not called by this app.

Run with the normal Blueprint workflow:

```text
!cargo bp wc3
run wc3
```

The launcher image is read from
`/common/Warcraft III/Warcraft III.exe` and must have SHA-256
`5a8cca727c719ae054adf8d15523a8e3099745225e2f4f9885f88774aa6f36d9`.
