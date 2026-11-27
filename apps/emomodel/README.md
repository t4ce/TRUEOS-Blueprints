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
emomodel reel
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
Roughness is fixed at 100% while the key light sweeps across the posed skin.
The bind-space roughness uses a 64-entry lighting ramp in the reel atlas.

`reel` plays the motion reel with the original diffuse lighting. The reel
plays Greeting, Nod, Squat, Spin, Model Pose, Show Full Body,
Shoot, and Peace Sign, then takes two short steps into the opening Greeting
crouch with both feet and fingertips close to the floor. The correction eases
out during takeoff, and a smaller height adjustment settles the landing. The
motion files are installed privately on the rig.

`greeting` opens the locally retargeted `AvatarSample_B_Greeting.glb` under
`/apps/emomodel`, or under `--root` when supplied. It opens the body lighting
study. The camera and body pose hold for an eight-second light sweep. With the
local clip, the avatar
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
generated 64 × 64 diffuse lookup that refreshes as the light moves. Two
1024 × 1024 RGB normal atlases from AvatarSample_B add face and body detail.
Startup decodes them to RGBA8 and samples them at the original skin UVs in
bind space. Playback skins the adjusted vertex normals through the existing
diffuse pass. The hair, eye and mouth geometry, and irises sample solid
palette colors.

`base-color` previews the original 4096 × 4096 RGBA face and body atlases
on the animated avatar. It samples the painted skin and mouth per fragment
through the unlit retained shader. The iris uses the flat palette because its
authored material requires alpha blending. Both atlases are decoded once and
occupy 128 MiB of texture residency together.

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

`autobs` is the automatic observer for the animated preview. In the lighting
study, it pans toward the landing stance for Nod and returns to the opening view
during the reverse jump. The reel fits all eight motions in one camera framing.
Soft black contact marks show nearby heels, toes, and fingertips. Each pose
projects contacts onto the ground plane and composites overlaps from one R8
brush into the material atlas. In the lighting study, reverse playback reuses
the baked contact masks. The reel's opening crouch registers one fingertip on
each hand.
After UI4 commits a resize, the observer eases along a shallow arc while looking
at the avatar.
It moves in for a larger area and pulls back more slowly for a smaller area.
Its orthographic framing keeps the avatar proportional and fits the animated
silhouette to the new aspect ratio. A new resize starts from the last drawn
camera pose. The trace records several confirmed camera frames per resize,
alongside the existing timing reports.

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
