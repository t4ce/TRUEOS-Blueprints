# Frog weather

Frog prints live weather for a selected point. Host usage: `Frog <longitude> <latitude>`.
Without arguments or a start script it uses the existing Holzminden coordinates.

On TRUEOS, Frog consumes the one-shot `vFile:launch` start script before fetching
weather. Each non-empty line is a command: `weather <longitude> <latitude>`.
Commands run in order, then Frog releases its terminal and shuts down. OSM's
context menu supplies this script with the frozen right-click coordinates.
Longitude comes first. Values must be finite and within ±180/±90 degrees.
Reverse geocoding supplies a name; the forecast always uses the selected point,
including when reverse geocoding fails.
