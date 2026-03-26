# Volta-Renaissance Phase 4: Inline PTX MMA + XOR Swizzle Design

> **Project Thesis**: Exploit V100-specific features (ITS, quadpair m8n8k4,
> unified L1/SMEM) that newer architectures abandoned.

## Why Phase 4

Phase 3 (Split-D) eliminated register spilling for ncols<=16 within WMMA.
But WMMA is a black box — we can't control:
1. SMEM address generation (XOR swizzle requires non-strided addressing)
2. MMA instruction scheduling (can't interleave with global loads)
3. Register allocation per quadpair (WMMA picks for us)

Phase 4 replaces WMMA with inline PTX to gain full control over the V100's
unique microarchitecture.

## V100-Specific Advantages to Exploit

### Independent Thread Scheduling (ITS)
Volta uniquely allows threads within a warp to diverge and reconverge at
arbitrary points. On Pascal: all 32 threads execute in lockstep. On Ampere:
hardware async (cp.async, TMA) handles overlap. On Volta: ITS is the
mechanism for software pipelining.

**Exploitation**: Interleave MMA instructions with global memory loads within
the same warp. While quadpair 0-1 executes MMA on tile N, quadpair 2-3 loads
tile N+1 from global memory. This is the "multi-stage software pipeline"
from the issue #500 spec.

### Quadpair m8n8k4 MMA
V100's MMA atom operates on 8 threads (quadpair), not 32 (full warp like
Ampere). This gives 4 independent scheduling points per warp vs 1.

**PTX instruction**: `mma.sync.aligned.m8n8k4.row.col.f32.f16.f16.f32`
- Input A: 2 half2 values per thread (row-major)
- Input B: 2 half2 values per thread (col-major)
- Output C: 8 float values per thread
- 8 threads cooperate per m8n8k4 tile

**Register layout per thread in quadpair**:
```
Thread T in quadpair Q (T = 0..3, Q = 0..1):
  A[0]: half2 at row (Q*4 + T), cols 0-1
  A[1]: half2 at row (Q*4 + T), cols 2-3
  B[0]: half2 at col (Q*4 + T), rows 0-1
  B[1]: half2 at col (Q*4 + T), rows 2-3
  C[0..7]: 8 floats covering 8x8 output tile
```

### XOR Swizzle (now possible without WMMA)
With inline PTX, we control SMEM addresses directly:
```cuda
// XOR swizzle for 8x8 FP16 tiles (V100: 32 banks x 4B)
__device__ int swizzle(int row, int col) {
    int bank_group = col >> 3;  // 8 halves per bank group
    int within = col & 7;
    return ((row ^ bank_group) << 3) | within;
}
```
Apply to all SMEM loads/stores. Expected: 2x SMEM throughput (measured on
Ampere, should transfer to V100 which has same bank geometry).

## Implementation Plan

### Step 1: Standalone m8n8k4 GEMM kernel
Write a minimal CUDA kernel that performs 8x8x4 MMA via inline PTX:
```cuda
asm volatile(
    "mma.sync.aligned.m8n8k4.row.col.f32.f16.f16.f32 "
    "{%0,%1,%2,%3,%4,%5,%6,%7}, {%8,%9}, {%10,%11}, "
    "{%12,%13,%14,%15,%16,%17,%18,%19};"
    : "=f"(d0),"=f"(d1),"=f"(d2),"=f"(d3),
      "=f"(d4),"=f"(d5),"=f"(d6),"=f"(d7)
    : "r"(a0),"r"(a1), "r"(b0),"r"(b1),
      "f"(c0),"f"(c1),"f"(c2),"f"(c3),
      "f"(c4),"f"(c5),"f"(c6),"f"(c7)
);
```
Verify correctness against cuBLAS GEMM.

### Step 2: XOR-swizzled SMEM load/store
Replace WMMA load_matrix_sync with manual loads through swizzled SMEM:
```cuda
// Load 8x4 tile from SMEM into registers for MMA
half2 a_frag[2];
int smem_row = threadIdx.x % 4 + (threadIdx.x / 4) * 4;
int smem_col = k_offset;
a_frag[0] = *(half2*)(smem + swizzle(smem_row, smem_col));
a_frag[1] = *(half2*)(smem + swizzle(smem_row, smem_col + 2));
```

### Step 3: Integrate into FA kernel
Replace the WMMA-based KQ computation in fattn-wmma-f16.cuh with:
1. XOR-swizzled Q store to SMEM
2. Manual register loads of Q and K fragments
3. Inline PTX m8n8k4 MMA
4. Manual store of KQ results to SMEM

### Step 4: Software pipeline with ITS
Add 2-stage pipeline:
- Stage 0: Load K[n+1] from global to registers
- Stage 1: MMA on K[n] with Q (already in registers)
- ITS allows stages to overlap within the same warp

### Validation
- cuobjdump: target REG <= 190 for all ncols variants
- vr-safe-test on GPU 0 (isolated)
- Bit-exact comparison against WMMA baseline (same SMEM layout, same accumulation order)
- llama-bench PP2048/TG128 comparison

## Risk Assessment
- **HIGH**: Inline PTX is fragile — register allocation must be exact
- **MEDIUM**: ITS pipeline may not improve on V100 if MMA latency is already hidden
- **LOW**: XOR swizzle is well-understood, 2x SMEM throughput is near-certain

## Dependencies
- CUTLASS SM70 MMA atom reference: /home/atari2036/gen-ai/cutlass/include/cute/arch/mma_sm70.hpp
- FastAttention layout reference: arXiv:2410.16663
- V100 tuning guide: Volta Independent Thread Scheduling section
