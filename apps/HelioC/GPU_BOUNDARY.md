# HelioC GPU boundary

The current executable reaches the persistent-resource admission rung:

```text
Helio-authored Cloud Engine WGSL
        -> exact parameter/resource contract
        -> isolated VMX GPUVM
        -> retained volume A/B plus parameter allocations
```

The following boundary is intentionally still cold:

```text
cloud-engine.trueos.helio native stages
        -> authenticated 3D sampled/storage state
        -> 4 x 4 x 4 simulation dispatch
        -> compute-write to fragment-sample visibility
        -> fullscreen three-vertex UI4 pass
```

The older `cpp cloud-high-wisps` output is neither a fallback nor an acceptance
oracle. HelioC must originate from the retained 3D simulation and raymarch
authored by the existing Helio Cloud Engine.
