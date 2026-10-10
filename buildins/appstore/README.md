# App Store start scripts

App Store consumes `vFile:launch` before opening its interactive UI. Each line
is `pull NAME_OR_ID` or `launch NAME_OR_ID`; blank lines and comments are ignored.
It resolves the catalog entry, downloads and verifies the blueprint, and launches
it through `trueos::vshell::launch_with_script`.

To supply the downloaded app with its own one-shot start script, add ` -- ` and
one command. For example, OSM submits:

```text
launch --sh3 Frog -- weather 13.8278 51.4713
```

App Store forwards `weather 13.8278 51.4713\n` to Frog's `vFile:launch`.
`--sh3` creates one kernel-owned Shell3 window and enters the downloaded app's
Matrix slot through the normal `§slotid` path. The flag belongs before the app
selector; it is not part of the child's script. Without it the launch inherits
the caller's Shell3 frontend, including SSH. UI4 callers can start the pullbot
with `launch_with_destination("appstore", script, LaunchDestination::Headless)`.
Existing requests without a forwarded command still launch with an empty script.
Multiple request lines retain their order and duplicates; a bad request does not
prevent later requests from being processed.
