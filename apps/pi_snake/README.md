# Pi Snake

`pi_snake` is an immediate Shell2 terminal-lease Blueprint. Launch it with:

```text
TAB
start pi_snake
```

The first local player starts with `3`, moves with WASD or arrows, and eats red
`@` apples. An apple pauses movement: type the next character of Pi (the first
required character is the mandatory `.`), then press an arrow to continue or
wait three seconds. A wrong character removes one chain cell. Missing that
first dot turns the player into a small literal Pi chart; the next apple resets
it to `3` before it can grow again.

The Blueprint listens on TCP port `45329`. Browse to `http://<TRUEOS-IP>:45329/`
to join the only remote seat (P2). The HTTP endpoint also accepts a bare TCP
connection containing a single WASD/`3` byte for a minimal non-browser client.
