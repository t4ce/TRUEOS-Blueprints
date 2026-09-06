# Renderer ownership boundary

The Blueprint catalog contains supported Picasso-owned applications. Names
describe the actual renderer boundary, not historical demo provenance.

| Blueprint selector | Ownership and dependency boundary |
| --- | --- |
| `trueos-picasso-example` | Example application for the custom TRUEOS Picasso renderer. It is Picasso-owned and is not a Helio example. |
| `picasso` | Reserved for a Blueprint that actually represents the custom TRUEOS Picasso renderer. It is intentionally not assigned to an example application. |

The former Helio Churn and Portal Rooms Blueprint ports are retired and no
longer appear in the catalog. The `picasso` selector stays reserved for the
actual Picasso renderer.
