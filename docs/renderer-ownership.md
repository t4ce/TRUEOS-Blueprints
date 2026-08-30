# Renderer ownership boundary

The Blueprint catalog keeps Picasso-owned work separate from Helio integration
work. Names describe provenance, not just the UI or GPU path an application
uses.

| Blueprint selector | Ownership and dependency boundary |
| --- | --- |
| `trueos-picasso-example` | Example application for the custom TRUEOS Picasso renderer. It is Picasso-owned and is not a Helio example. |
| `helio_churn_trueos` | TRUEOS Blueprint port of Helio's Churn demo. Demo interaction and presentation live in `helio-examples/helio_churn_trueos`; no upstream Helio engine crate is linked at runtime. |
| `helio_portal_trueos` | TRUEOS Blueprint port of Helio's Portal Rooms demo. Demo interaction and presentation live in `helio-examples/helio_portal_trueos`; no upstream Helio engine crate is linked at runtime. |
| `picasso` | Reserved for a Blueprint that actually represents the custom TRUEOS Picasso renderer. It is intentionally not assigned to an example application. |

Helio is third-party software by Tristan Poland (`Trident_For_U`), copyright
2026, licensed under the MIT License. Its upstream repository is
[Far-Beyond-Pulsar/Helio](https://github.com/Far-Beyond-Pulsar/Helio). Its
license and authorship remain attached to the two explicitly named Helio
examples above.

The old kernel-resident `helio` Shell2 command was a TRUEOS migration/demo
launcher rather than the Helio WGPU renderer. It has been retired. Its two
retained examples are separate Blueprint applications: `helio_churn_trueos`
and `helio_portal_trueos`. Neither aliases `TRUEOS-Picasso-Example`; the
`picasso` selector stays reserved for the actual Picasso renderer.

The retired `HelioV` voxel integration and `HelioC` cloud integration are not
Picasso applications and have no Blueprint selectors. Helio's hosted cloud
prototypes remain in Helio. A future dynamic volume-cloud renderer belongs to
Picasso and must use workload-neutral TRUEOS texture/volume primitives rather
than aliasing either retired integration.
