### cd "TRUEOS Blueprints/hello_world_bp"
### cargo bp

That will 

```
build     the app against the local trueos wrapper crate
run       the local trueos-blueprint tool
produce   a .bp
```

for you.

## Your Blueprint is now hello_world_app.bp
Create as many Apps as you like!

### Run
Copy the .BP onto root Folder of primary File-System. <br/>
Use Shell Command **run**. <br/>
<br/>
It will yield a list, find your Apps ID, <br/>
**run <id>** will load your Blueprint and run whatever you decide. <br/>
<br/>

Must have
  **Rust nightly available**
  **7z, ld, objcopy, readelf** installed

## Host build path

For local iteration on a host OS, you can build and run the same wrapper-based app with:

### cd "TRUEOS Blueprints/hello_world_bp"
### cargo run --features host-std

That uses the `trueos` wrapper crate with its `host-std` backend instead of the kernel CABI.
The low-level `trueos-sys` crate remains in the workspace as internal ABI plumbing for the wrapper/tooling layer.
The first host backend slice is aimed at `vsys`, `vshell`, and `vfs`, with safe stubs for the rest.
