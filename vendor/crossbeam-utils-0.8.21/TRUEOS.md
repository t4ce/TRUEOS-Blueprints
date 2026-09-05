# TRUEOS integration

Based on crossbeam-utils 0.8.21 from the Cargo registry archive.

Exclude the Unix `JoinHandleExt` implementation on TRUEOS and the legacy zkvm
target: these targets do not expose pthread handles. Crossbeam's other utilities
remain available. Scoped thread creation still delegates to the standard thread
builder and inherits its Unsupported result on TRUEOS; native worker submission
remains an explicit application responsibility.

The Blueprint workspace and packer's platform overlay list both select this
directory, including when an application is built from a staged manifest.
