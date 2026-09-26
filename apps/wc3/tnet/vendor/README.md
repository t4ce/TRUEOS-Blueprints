# No vendored server source or binary

This archive contains the installer and control code, not PvPGN itself.
The installer obtains the pinned upstream revision listed in SOURCES.md and
installs dependencies from your configured Ubuntu APT repositories.

The packaging environment could read repository text but could not download
and compile the real source tree. No claim is made that this is an offline
bundle or that an actual Ubuntu installation was tested. Git objects and
build files are cached under /var/cache/w3box after installation. --no-apt
only skips package installation; a missing Git source still requires network.
