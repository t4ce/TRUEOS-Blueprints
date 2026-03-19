### Go into the app folder
cd "TRUEOS Blueprints/hello_world_bp"

### Build and pack
cargo bp

That will 

## build     the app against the local trueos and trueos-sys 
## run       the local trueos-blueprint tool
## produce   a .bp

# Your Blueprint is now hello_world_app.bp

Create as many Apps you would like, move them to the Root of your primary Mount and use the Shell Command "run".
It will yield a list, find your Apps ID, run <id> will load your Blueprint and run whatever you decide.

Must have
  Rust nightly available
  7z, ld, objcopy, readelf installed
