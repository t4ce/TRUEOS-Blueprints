# Kernel launcher retired

The former kernel-owned XPAPP launcher was deleted from TRUEOS. The kernel no
longer owns PE loading, import policy, launcher heap/event/thread/window/TLS
state, locale behavior, or CreateProcess probing.

`xpapp.bp` is the owner of the Warcraft III XP personality. TRUEOS exposes only
the XPAPP-private generic x86 carrier ABI and the optional Gate-0 carrier probe.

The historical bare-metal call trace remains an oracle in the repository
history and in the Blueprint personality tests; it is not an executable kernel
path.
