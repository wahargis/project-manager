# Phase 3: Split-D Tiling — Results

> **V100 Exploitation**: Register file architecture. 65536 regs/SM with 255 max/thread.
> Split-D keeps D_CHUNK fragments within budget, eliminating spilling at D=128.

## Implementation
Modified `ggml/src/ggml-cuda/fattn-wmma-f16.cuh`:
- Added `D_CHUNK = (Dk <= 64) ? Dk : 32` constant
- Q fragments `Q_b[D_CHUNK/16][ncols/frag_n]` instead of `Q_b[Dk/16][ncols/frag_n]`
- Q stays in SMEM; reloaded per D_CHUNK in the KQ inner loop
- Outer loop: `for (d_off = 0; d_off < Dk; d_off += D_CHUNK)`
- D<=64: no tiling (D_CHUNK=Dk), identical to baseline

## Register Profile (cuobjdump, D=128)

| Variant | Baseline REG | Baseline STACK | Split-D REG | Split-D STACK |
|---------|-------------|----------------|-------------|---------------|
| ncols=8 | 24 (capped) | 40B spill | **186** | **0** |
| ncols=16 | 24 (capped) | 40B spill | **248** | **0** |
| ncols=32 | 255 | 608B spill | 255 | 608B spill |

ncols=8 and ncols=16 achieved **zero register spilling**.
ncols=32 unchanged — accumulator arrays dominate, not Q fragments.

## Benchmarks (0.8B Q4_K_M, single V100 PCIe, 3 reps)

| Config | PP2048 (tok/s) | TG128 (tok/s) |
|--------|---------------|---------------|
| Baseline | 960 | 34.05 |
| D_CHUNK=32 | **976 (+1.7%)** | **34.05 (0%)** |
| D_CHUNK=64 | 976 (+1.7%) | 34.04 (0%) |

Split-D is **faster** on prefill due to register spill elimination.
Decode unchanged (decode kernel is vec_ext, not affected by Split-D).
D_CHUNK=64 offers no benefit over D_CHUNK=32 (SMEM reload is not bottleneck).

## Status
- VALIDATED and committed (feat/tq3_0 branch)
- Safe-tested via vr-safe-test on GPU 0 (isolated PCIe adapter)
- No GPU wedge, no Xid errors
- ncols=32 improvement deferred to Phase 4 (inline PTX)
