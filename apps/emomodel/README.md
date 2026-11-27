# emomodel

On-demand Blueprint that displays a humanoid mesh and can prepare Model2Vec
data for Lily's expression selector. Registered in `apps.json`; this is not a
buildin.

## TRUEOS

Build from the Blueprint repository:

```sh
cargo bp apps/emomodel
```

The existing SDK build command also publishes the resulting `dist/emomodel.bp`
to its configured app catalog. Install/launch using the normal Blueprint flow.
The Blueprint uses argv-v1 and TRUEOSFS scope. Commands:

```sh
emomodel
emomodel show
emomodel skin
emomodel skin --eyes nebula
emomodel reel
emomodel butterfly
emomodel greeting
emomodel base-color
emomodel status
emomodel mesh
emomodel setup
emomodel setup 8m
emomodel setup 32m
emomodel setup 4m
```

On TRUEOS, explicit `show` displays the original GLB's body, face, and hair in
their rest pose through the built-in flat-color vGPU pipeline. The app redraws
when UI4 delivers a final window extent. The avatar stays centered at its
original pixel size, with any overflow clipped by the frame.

Bare launch through `§§emomodel` opens the skin study. It uses
the motion reel when the seven locally retargeted files `motions/01.glb` through
`motions/07.glb` are installed, or the built-in Nod on a clean install.
Roughness is fixed at 100%. The key azimuth and the baked 2K sky rotate with
the observer camera. Sky diffuse and view-dependent sheen shade the skin
through a 64 × 32 color lookup in the reel atlas. Shell3 keeps the sky and
sheen levels and the selected key through the motion reel, dance, and the
butterfly gesture. White is the default. Sunrise uses the Long path color
from the [offline sunlight study](studies/sky/emomodel-sunlight-study.html).
Kelvin ranges from 9,000 K (cool) to 2,200 K (warm) and uses an equal-luminance
blackbody color curve.

Right-click in `skin` or `butterfly` to choose a pastel top or restore the original
appearance. The controls are also available in `reel` when its motion files are
installed. The shortest top ends above the navel and near the shoulders.
Adjust the avatar's left sleeve, right sleeve, and hem independently in 10%
steps. Each sleeve reaches its wrist at maximum length, and the longest hem
ends near the base of the seat. Press `T` to cycle colors, `[` or `]` for the left sleeve, `{` or
`}` for the right sleeve, and `-` or `=` to raise or lower the hem. Press
`Space` to roll a random pastel top and matching socks. Each roll picks a
shared sleeve length, a hem length, and a shared sock height. The next press
removes the outfit, and the one after rolls again. The neckline, sleeve
openings, hem, and sock cuffs follow rounded contours with
dark edges and two staggered rows of gray stitch dashes. The neckline trim is
half as wide as the sleeve and hem trim. A shallow procedural bump makes the
ribbons catch the light as the avatar moves.

The native motion reel starts at 4× MSAA on a supported Alder Lake-S GPU.
Use `--msaa 1` for single-sample rendering or `--msaa 2` to start at 2×.
Press `M` to toggle 2× MSAA or `Shift+M` to toggle 4× MSAA during playback.
Both modes support render targets up to 1280×720, with 1× used outside that limit.

The [offline MToon study](studies/mtoon/emomodel-mtoon-study.html) compares
the isolated shade-map difference, a directional toon ramp on flat skin, and
mapped outlines under one key light.
Open the texture inspector in the study to view and download the source maps.

The [offline firelight study](studies/sky/emomodel-fire-study.html) compares a
low-light reference with a moving light pattern. Strength, speed, mask scale,
contrast, and two colors can be tuned independently.

In `skin` and `reel`, running a patch in Lico switches EmoModel into
graph-driven dance. Lico publishes a `DanceClueSnapshot` with the active DSP
connections and scheduled cues. EmoModel uses that information to shape its
steps, weight shifts, and upper-body gestures. Lico Stop blends back to where
the previous motion paused.

`reel` plays the motion reel with the same sky and a smooth diffuse key. The reel
plays Greeting, Nod, Squat, Spin, Model Pose, Show Full Body,
Shoot, and Peace Sign, then takes two short steps into the opening Greeting
crouch with both feet and fingertips close to the floor. The correction eases
out during takeoff, and a smaller height adjustment settles the landing. The
motion files are installed privately on the rig.

`butterfly` opens a front-on preview of the drawing gesture. The avatar traces
the `bfly` outline and both inner wing strokes in the former heart's drawing
plane. The sketch spans 0.87 meters in model space. The top-row `3` plays the
gesture in the current camera view. Each completed sketch adds one live
butterfly in the second-largest `bfly` size tier. Each uses either Key 5 wing
design, keeps its rolled inset color, and follows its own flight path. It
shrinks through smaller tiers as it descends off the bottom of the frame.
When the first butterfly appears, EmoModel reads the current opaque display
color and uses Bfly's easing to settle on the nearest palette swatch. White
and gray ease toward green. The transparent avatar surface reveals the solid
display color.

`greeting` opens the locally retargeted `AvatarSample_B_Greeting.glb` under
`/apps/emomodel`, or under `--root` when supplied. With the local clip, the avatar
plays the Greeting jump, lowers its arms, nods once, raises its arms back to
the landing pose, and plays Greeting in reverse. The full sequence repeats.
A clean install alternates Nod with 15 seconds of varied wind and five seconds
of calm. Animation credits to pixiv Inc.'s VRoid Project. Install Greeting
privately from VRMA_02 in the official VRMA Motion Pack.

The lighting study bakes positions and authored skin normals at 45 Hz during
startup. The reel samples its current pose and reuses one set of draw buffers.
The Picasso draws include body skin, face skin, eyes, mouth,
and hair. The face holds its neutral expression and follows the head joint
during playback. Each frame draws lit skin and flat-color parts from the same
retained pose with shared depth and back-face culling. The skin samples a
generated 64 × 64 lookup that refreshes with the camera and sky levels. Two
1024 × 1024 RGB normal atlases from AvatarSample_B add face and body detail.
Startup decodes them to RGBA8 and samples them at the original skin UVs in
bind space. Playback skins the adjusted vertex normals through the existing
diffuse pass. The hair and mouth geometry sample solid palette colors.

The animated previews use the irises from [the Lily sprite atlas](assets/lily-eyes.png)
with AvatarSample_B's sclera and eye decorations. The crossed and uncrossed
Lily portraits supplied the baseline eye shape and violet palette. The atlas
contains three 512 × 512 RGBA cells, in order from left to right. Each cell is
drawn twice, with one eye mirrored in the renderer. On an animated launch,
TRUEOS picks one style at random and keeps it for both eyes throughout the run.
Pass `--eyes` to pick a specific style.

| `--eyes` | Style |
| --- | --- |
| `amethyst` | Electric violet and magenta fibers |
| `nebula` | Indigo and cyan with a prismatic lower arc |
| `doll` | Soft lavender, pink, and glassy reflections |

Each iris has a black pupil overlay fitted to the artwork. Its four stages
cover 1×, 2×, 3×, and 4× the neutral pupil area. The pupil eases between
animation cues in the motion reel, built-in Nod, Greeting, Butterfly, and Lico
dance. The eyelash and decoration cutouts stay in front of both sprite layers.

The lit avatar gets its ear color from the skin mesh. The
[ear height map](assets/AvatarSample_B-ear-height.png) adds soft folds to the
front ear normals. Both ears sample the same texture, using reflected UVs on
the opposite side.

The default skin preview also samples a [32 × 32 belly normal crop](assets/AvatarSample_B-belly-normal.png)
from the authored body atlas. It adds the original navel relief to the nearby
vertex normals with two short folds and fades into the surrounding skin.
The lit preview samples [body tint masks](assets/AvatarSample_B-body-tint.png)
on the original body UVs. One mask shades the navel, and a mirrored grayscale
reference crop shades the two chest details on exposed skin. The lighting
study uses the full body normal atlas and the same two folds.

`base-color` previews the original 4096 × 4096 RGBA face and body atlases
on the animated avatar. It samples the painted skin and mouth per fragment
through the unlit retained shader. Both atlases are decoded once and occupy
128 MiB of texture residency together.

In the animated preview, 28 procedural guides take the place of the long
back hair. The source surface provides the roots, segment lengths, the direction
each guide leaves the scalp, and its side-to-side spread. The scalp, bangs, and
shorter locks still use the GLB mesh. Each guide has eight fixed-length segments.
The roots follow the head, and the free lengths settle into a straighter hang
under gravity. The idle wind pushes mostly sideways and toward the avatar's
front, with softer breezes from other directions. The guides slide over the
posed body and keep moving as the wind falls calm. The motion is baked into
45 Hz frames at startup. The reverse Greeting shares forward body poses and
simulates hair in playback order. The crouched ends of the loop blend into the
same drape. Idle frames share their settled body pose. The primary guides draw
as tapered ribbons in the same flat hair color, about 3 to 4 pixels wide at the
default preview size.

`autobs` holds a fixed camera throughout each playback mode. The frame reserves
room for the full motion, including raised and outstretched hands. Butterfly and
Lico Dance use their own framing when active. The limiting window axis leaves
at least four pixels of clearance around the motion bounds.
Soft black contact marks show nearby heels, toes, and fingertips. Each pose
projects contacts onto the ground plane and composites overlaps from one R8
brush into the material atlas. In the lighting study, reverse playback reuses
the baked contact masks. The reel's opening crouch registers one fingertip on
each hand.
After UI4 commits a resize, the observer eases along a shallow arc while looking
at the avatar.
It moves in gradually for a larger area. When space shrinks, it fits the
motion bounds immediately and eases the viewing angle back. Its orthographic
framing keeps the avatar proportional. A new resize starts from the last drawn
camera pose. The trace records confirmed camera frames per resize alongside the
existing timing reports.

`shaders/retained-unlit.bundle` combines the stock `churn-forward`,
`helio-retained-transform`, and `picasso-textured` packages from
`TRUEOS-Picasso/shaders/trueos`, in package ID order.

On the host, bare launch defaults to `status`, which reports installed bundles
without network or filesystem writes. Run `setup` to prepare an expression
model. It defaults to `auto`, which reuses a valid bundle or tries 8m, 32m, and
4m in order. An explicit ID tries only that model.

## Embedded humanoid

The Blueprint embeds [AvatarSample_B.glb](assets/AvatarSample_B.glb). `emomodel
mesh` installs it from the executable without network access. `setup` also
installs it before preparing the expression model. Both use `--root` when
provided. The default path is `/apps/emomodel/AvatarSample_B.glb`.

The 1,666,020-byte GLB preserves all 32,847 triangles in three meshes and
seven material sections. It retains the skinning data and the 54 humanoid
node mappings in `extras.humanoidBones`. It uses plain materials and contains
zero textures, facial morph targets, or animation clips. The source VRM
contains a joint hierarchy and bind data for posing but no glTF animation
tracks. The body and face skin sections total 14,254 triangles and can be
selected for a body-only preview.

The source is [AvatarSample_B by VRoid Project](https://hub.vroid.com/en/characters/7939147878897061040/models/2292219474373673889).
The extracted asset is distributed under the [VRoid sample terms](https://vroid.pixiv.help/hc/en-us/articles/4402394424089-VRoidPreset-A-Z).

## Expression model bundles

| ID | Pinned model | Complete bundle bytes | Role |
| --- | --- | ---: | --- |
| 8m | minishlab/potion-base-8M | 30,920,628 | Original working default |
| 32m | minishlab/potion-base-32M | 130,703,808 | Larger alternative |
| 4m | minishlab/potion-base-4M | 15,802,292 | Smaller fallback |

These are compatible static embedding models; larger does not guarantee a
better Lily choice, and 4m is not claimed to have equal general accuracy.
All three returned the same expected five labels in the small host check below.

## Files and consumer handoff

Data lives under the app's own TRUEOSFS root:

```text
/apps/emomodel/
  active.json
  AvatarSample_B.glb
  setup.lock
  8m/<pinned-revision>/
    config.json
    tokenizer.json
    model.safetensors
    ready
```

The other options have their own revision directories. `active.json` contains
the selected ID, repository, pinned revision, directory, and the three filenames.
Consumers read this manifest and load that directory, without fetching from
Hugging Face or relying on a host cache. Each model requires all three files;
weights alone are insufficient. The existing selector still separately needs
its expression examples (`lily2Vec/emotions.json`).

`models.json` pins upstream revisions, file sizes, and SHA256 digests. Native HTTPS uses the kernel bounded HTTP/TLS fetch service (90 seconds,
a body limit of at least 1 MiB for redirect responses, with exact artifact
size and SHA256 verification afterward), matching Apps. Host HTTPS uses reqwest/Hyper/rustls
with bounded redirects.
Each response is limited to its expected size, then checked before publication.
Verified files can be reused after interruption; the `ready` marker is written
last. Status rechecks the file hashes rather than accepting file existence.
An exclusive setup lock prevents concurrent installers. TRUEOSFS uses its
whole-file write path, without depending on replacement rename semantics.
The old active manifest is only updated after a complete bundle succeeds.

Downloads buffer one artifact at a time: allow memory for the largest weights
file (about 129 MB for 32m), plus network/runtime overhead. This installer is
short lived. It is not the resident inference footprint.

## Host verification

Build without publishing, and use a host directory instead of `/apps`:

```sh
cargo build --release --manifest-path apps/emomodel/Cargo.toml
target/x86_64-unknown-linux-gnu/release/emomodel --root /tmp/emomodel status
target/x86_64-unknown-linux-gnu/release/emomodel --root /tmp/emomodel setup 8m
cargo test --manifest-path apps/emomodel/Cargo.toml
cargo clippy --manifest-path apps/emomodel/Cargo.toml -- -D warnings
```

Use the printed directory with the original experiment:

```sh
/home/t4ce/Repositories/lily2Vec/target/release/lily2vec \
  --model /tmp/emomodel/8m/bf8b056651a2c21b8d2565580b8569da283cab23 \
  --scores "i love you"
```

Verified locally: all three bundles downloaded and passed integrity checks;
each loaded with `lily2Vec` and returned `wave`, `sad`, `affection`,
`disapprove`, `agree` for `hi`, `i dont feel well`, `i love you`, `NO`,
`yes that will do`. Repeated setup reused valid files without downloads.
A deliberately corrupted tokenizer was reported invalid and repaired by setup.
Unit tests and Clippy passed. The native Blueprint packaged successfully
(2,155,452 bytes) and passed SDK loader/CABI compatibility checks. Native execution was attempted over SSH on 192.168.178.94. The original
reqwest path stalled fetching weights; it was stopped and replaced by the
kernel fetch service. After switching native network requests to the kernel fetch service, using
`?download=true` for Hugging Face downloads, and publishing files with
`trueos::async_fs::write_file`, setup completed on that machine: 8m ready
in 7.806 seconds. Weights and tokenizer were downloaded and hash-verified;
the native completion marker and active manifest were published.

Single host runs, cached files, five short inputs (not a quality benchmark):

| Model | Model + example load | Warm embedding + selection per input |
| --- | ---: | ---: |
| 8m | 43.8 ms | 0.005–0.007 ms |
| 32m | 127.2 ms | 0.009–0.011 ms |
| 4m | 24.3 ms | 0.004–0.007 ms |

## Inference placement

The existing selector performs CPU vector lookup/pooling and a tiny similarity
scan. The SDK can package this setup utility in its own VMX/Hull; no persistent
VMX is needed for data preparation. Bringing inference to TRUEOS is a separate
integration: the host Model2Vec/tokenizer dependencies have not been ported by
this change.

Codex can eventually load the selected data in its own Hull, keeping one warm
model per process, or a separate on-demand inference Blueprint can share one
warm model among clients. SQLite remains a database service.

The current `probes/xerj_service` launches the engine with no Potion model or
emotion-selector configuration. XERJ supports lexical/neural embedding modes,
but we found no Model2Vec/Potion implementation in the engine crates searched.
No XERJ inference latency, memory sharing, or semantic benefit was measured
here, so this change does not move expression inference into xerj. A measured
comparison and a compatible embedding/selection interface would precede that
decision. No engine or SQLite code was changed.
