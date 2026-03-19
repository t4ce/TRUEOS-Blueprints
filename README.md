### Go into the app folder
cd "TRUEOS Blueprints/hello_world_bp"

### Build and pack
cargo bp

That will 
```
build     the app against the local trueos and trueos-sys 
run       the local trueos-blueprint tool
produce   a .bp
```
for you.

## Your Blueprint is now hello_world_app.bp

Create as many Apps as you like!

### Run
Copy the .BP them into Rood Folder of primary Filesystem Mounted. <br/>
Use the Shell Command "run". <br/>
<br/>
It will yield a list, find your Apps ID, <br/>
run <id> will load your Blueprint and run whatever you decide. <br/>
<br/>
Must have
  Rust nightly available
  7z, ld, objcopy, readelf installed
