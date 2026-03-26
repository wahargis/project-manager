# Phase 1: Existing fattn-wmma Kernel Analysis
> **Project Thesis**: Volta-Renaissance is architectural specialization TO the NVIDIA V100 (SM70),
> not a port of newer techniques downward. Performance gains come FROM exploiting V100-specific
> features that newer architectures abandoned or changed. Every design decision asks:
> what does V100 do differently, and how can we exploit that?


## Source: ggml/src/ggml-cuda/fattn-wmma-f16.cuh (542 lines)

### Template Parameters
```
flash_attn_ext_f16<Dk, Dv, ncols, nwarps, VKQ_stride, parallel_blocks, KQ_acc_t, use_softcap>
```
For D=128: Dk=128, Dv=128, ncols=4 (decode) or 16+ (prefill)

### MMA Strategy
- Uses WMMA API: nvcuda::wmma::fragment with m16n16k16 tiles
- V100 decomposes m16n16k16 into multiple m8n8k4 hardware atoms internally
- NO explicit control over SM70 instruction scheduling
- NO inline PTX -- all through WMMA abstraction

### Register Profile (from cuobjdump)
| Kernel | Registers | Stack | SMEM | Occupancy |
|--------|-----------|-------|------|-----------|
| Decode (vec_ext, ncols=1) | 244 | 0 | 640B | 8 warps = 12.5% |
| Prefill (ext_f16, ncols=16+) | 24 | 40B spill | dynamic | high but spilling |

**Decode**: 244 regs = 95.7% of 255 limit. Zero spill. Max occupancy: 65536/(244*32) = 8 warps.
**Prefill**: Compiler capped at 24 regs, spills 40 bytes to local memory. Q fragments Q_b[Dk/16][ncols/frag_n] = Q_b[8][N] all resident -- massive register pressure at D=128.

### SMEM Layout
- Padding: Dk_padded = Dk + 8 (simple +8 padding, NOT XOR swizzle)
- +8 half elements = 16 bytes = 4 bank offsets. Reduces but does not eliminate conflicts.
- Single shared buffer for KQ and VKQ phases (cannot overlap)
- KQ: ncols * kqs_padded * sizeof(KQ_acc_t)
- VKQ: VKQ_ratio * ncols * Dv_padded

### D-Dimension Handling
NO tiling across D. Inner loop:
```cuda
for (int k_KQ_0 = 0; k_KQ_0 < Dk; k_KQ_0 += 16) {
    wmma::load_matrix_sync(K_a, K_h + ..., stride_K);
    for (int j = 0; j < ncols/frag_n; ++j) {
        wmma::mma_sync(KQ_c[j], K_a, Q_b[k_KQ_0/16][j], KQ_c[j]);
    }
}
```
All D/16 = 8 Q fragments pre-loaded and resident. This works at D=64 (4 fragments) but at D=128 (8 fragments) it overflows the register budget.

### Optimization Opportunities

1. **XOR Swizzle (Phase 2)**: Replace +8 padding with XOR address permutation. Expected: 2x SMEM throughput based on Ampere measurements. Low risk -- only changes address computation, not algorithm.

2. **Split-D Tiling (Phase 3)**: Instead of pre-loading all 8 Q fragments, load D_chunk=32 or 64 at a time. Reduces live registers from 8*frag_size to 2-4*frag_size. Accumulate partial KQ sums in register-resident FP32. Requires restructuring the inner loop.

3. **Inline PTX MMA (Phase 4)**: Replace WMMA with explicit m8n8k4 PTX. Gains:
   - Direct control over register allocation per quadpair
   - Custom SMEM-to-register load scheduling
   - Interleave MMA with global loads (software pipeline)
   Loss: WMMA portability (but we are targeting V100 specifically)

4. **Double-buffered SMEM (Phase 4)**: Currently single buffer alternates KQ/VKQ. With 2 buffers: load next K tile while computing current -- hide global memory latency.

### Baseline Performance (Phase 1 measurements)
- 0.8B Q4_K_M on single V100 PCIe: PP2048=960, TG128=34 tok/s
- 35B-A3B on NVLink pair: PP2048=705-824, TG128=83-97 tok/s
- 35B at 252K context: TG1024=8.42 tok/s (10x degradation from 2K)

### Next Steps
- Phase 2: Implement XOR swizzle in SMEM layout (lowest risk, highest ROI)
- Phase 3: Prototype Split-D tiling to eliminate register spilling at D=128
- Measure with cuobjdump after each change to verify register reduction
