# Renderer ownership boundary

The Blueprint catalog keeps Picasso-owned work separate from Helio integration
work. Names describe provenance, not just the UI or GPU path an application
uses.

| Blueprint selector | Ownership and dependency boundary |
| --- | --- |
| `trueos-picasso-example` | Example application for the custom TRUEOS Picasso renderer. It is Picasso-owned and is not a Helio example. |
| `helio-example` | Reserved for a separate Blueprint backed by actual Helio code. It is deliberately not registered until that Blueprint exists. |
| `picasso` | Reserved for a Blueprint that actually represents the custom TRUEOS Picasso renderer. It is intentionally not assigned to an example application. |
| `HelioV` | TRUEOS platform integration for the real Helio renderer and SceneDB. It retains explicit Helio crate dependencies. |
| `HelioC` | TRUEOS platform integration for Helio's Cloud Engine workload. It retains an explicit Helio dependency and the upstream workload as its oracle. |

Helio is third-party software by Tristan Poland (`Trident_For_U`), copyright
2026, licensed under the MIT License. Its upstream repository is
[Far-Beyond-Pulsar/Helio](https://github.com/Far-Beyond-Pulsar/Helio). Helio's
license and authorship continue to govern Helio code consumed by `HelioV` and
`HelioC`; those Blueprints do not transfer that code into Picasso ownership.

The old kernel-resident `helio` Shell2 command was a TRUEOS migration/demo
launcher rather than the Helio WGPU renderer. It has been retired. A future
Blueprint-side successor may use the `helio-example` selector only when its
implementation is genuinely Helio-backed. It must never alias
`TRUEOS-Picasso-Example`; the `picasso` selector stays reserved for the actual
Picasso renderer.
