# V100 Independent Thread Scheduling (ITS): Pipeline Research for FlashAttention

> **Project**: Volta-Renaissance Phase 4 -- Inline PTX MMA + Software Pipelining
> **Date**: 2026-03-26
> **Scope**: SM70/Volta ONLY -- no SM80+ techniques

---

## 1. How ITS Works on SM70/Volta

### 1.1 The Fundamental Change from Pascal

**Pascal (SM60/61) -- Lockstep Execution:**
- Single program counter (PC) shared by all 32 threads in a warp
- Single active mask tracks which threads are active
- Divergent branches execute serially: if-branch for masked threads, then else-branch
- Reconvergence is implicit and immediate at the post-dominator
- All threads in the warp always issue the same instruction or are masked off

**Volta (SM70) -- Independent Thread Scheduling:**
- **Per-thread program counter and call stack** (2 registers per thread consumed for PC state)
  - At full theoretical occupancy (2048 threads/SM), 16 KB of the 256 KB register file
    is consumed by per-thread PC state
- A **schedule optimizer** dynamically groups active threads within the same warp
  that share the same PC into temporary SIMT execution groups
- Threads can diverge and reconverge at **sub-warp granularity**
- Divergent branches can be **interleaved** rather than executed serially
- Reconvergence is **not guaranteed** -- explicit synchronization is required

**Ampere (SM80) -- Hardware Async:**
- Retains per-thread PC from Volta
- Adds cp.async for asynchronous global-to-shared memory copies
- Adds mbarrier for async barrier signaling
- Warp specialization becomes less necessary because cp.async allows the
  same warp to overlap memory and compute without ITS tricks

**Key insight**: Volta sits in a unique position -- it has the sub-warp scheduling
flexibility of ITS but lacks the hardware async primitives of Ampere. ITS-based
software pipelining is the **only** mechanism for overlapping global loads with
tensor core compute on SM70.

### 1.2 The Schedule Optimizer

From the Volta whitepaper and Hot Chips 2017 (Choquette):

> "A schedule optimizer determines how to group active threads from the same
> warp together into SIMT units. This retains the high throughput of SIMT
> execution but with much more flexibility: threads can now diverge and
> reconverge at sub-warp granularity."

The schedule optimizer is a hardware unit that:
1. Examines per-thread PCs within a warp
2. Groups threads that share the same PC into execution cohorts
3. Issues these cohorts as SIMT units to the execution pipelines
4. Can interleave different cohorts from the same warp across cycles

**Granularity of scheduling**: The schedule optimizer works at the warp level
(32 threads) but can form sub-warp groups. The minimum scheduling unit is
not publicly documented by NVIDIA, but empirical evidence from the Jaccard
Weights paper (Besta et al., 2018) and practical CUDA work suggests it
operates at approximately 8-thread (quadpair) granularity -- which aligns
perfectly with the m8n8k4 MMA atom.

### 1.3 What ITS Does NOT Do

ITS does not:
- Eliminate the performance cost of divergence (still fewer active threads per cycle)
- Allow truly independent per-thread scheduling (threads are still grouped by PC)
- Guarantee any particular interleaving order (it is an optimization, not a contract)
- Enable preemption or priority between threads

ITS does:
- Allow the hardware to interleave execution of divergent branches
- Enable intra-warp producer/consumer patterns via intentional divergence
- Remove the strict requirement that divergent paths execute serially

---

## 2. PTX Instructions for ITS-Based Pipelining

### 2.1 __syncwarp (PTX: bar.warp.sync)

Introduced specifically for Volta to handle ITS:

```ptx
// Synchronize all active threads in the warp
bar.warp.sync 0xffffffff;

// Synchronize a subset of threads (mask-based)
bar.warp.sync <membermask>;
```

In CUDA C++:
```cuda
__syncwarp(0xffffffff);     // Full warp sync
__syncwarp(0x000000ff);     // Sync threads 0-7 (one quadpair)
```

**Critical for ITS**: Pre-Volta, warp-level data exchange through shared memory
worked because lockstep execution guaranteed visibility. On Volta, threads may
be at different PCs, so `__syncwarp()` is **required** to ensure data written
by one thread is visible to others in the same warp.

**Implementation note**: `__syncwarp()` is implemented via registers (not
barriers), and there is only one warp-level "barrier" -- diverged threads can
call `__syncwarp()` at different PCs and still converge. This is fundamentally
different from `__syncthreads()` which uses barrier instances (16 per CTA)
and requires all threads to reach the same barrier instance.

### 2.2 bar.sync (PTX block-level barrier)

```ptx
// Block-level barrier -- all threads in CTA must arrive
bar.sync 0;

// Named barriers (up to 16 per CTA on Volta)
bar.sync <barrier_id>;  // barrier_id in 0..15

// Named barrier with thread count
bar.sync <barrier_id>, <thread_count>;
```

Named barriers with thread counts enable **warp specialization** -- different
groups of warps synchronize independently:

```cuda
// Producer warps signal completion
if (warp_id < NUM_PRODUCER_WARPS) {
    // ... load data ...
    asm volatile("bar.sync 1, %0;" :: "r"(NUM_PRODUCER_THREADS));
}
// Consumer warps wait for producers
if (warp_id >= NUM_PRODUCER_WARPS) {
    asm volatile("bar.sync 1, %0;" :: "r"(NUM_PRODUCER_THREADS));
    // ... consume data ...
}
```

### 2.3 nanosleep (PTX)

Available on SM70+:
```ptx
nanosleep.u32 <timeout_ns>;
```

In CUDA C++:
```cuda
__nanosleep(ns);
```

Suspends a thread for approximately `timeout_ns` nanoseconds, yielding its
scheduling slot. Useful for:
- Reducing contention in spin-wait loops
- Yielding producer threads that are ahead of consumers
- Fine-grained back-pressure in software pipelines

**Caveat**: The actual sleep duration is approximate and implementation-defined.
Not suitable for precise timing, but useful for reducing wasted issue slots.

### 2.4 mma.sync.aligned (Tensor Core)

```ptx
mma.sync.aligned.m8n8k4.row.col.f32.f16.f16.f32
    {%d0, %d1, %d2, %d3, %d4, %d5, %d6, %d7},
    {%a0, %a1},
    {%b0, %b1},
    {%c0, %c1, %c2, %c3, %c4, %c5, %c6, %c7};
```

The `.sync` qualifier is critical -- see Section 5 for detailed analysis.

### 2.5 Memory Fence Instructions

```ptx
membar.cta;    // Fence: all prior writes visible within CTA
membar.gl;     // Fence: all prior writes visible globally
membar.sys;    // Fence: all prior writes visible system-wide
```

For SMEM-mediated data exchange within a CTA, `membar.cta` (or `__threadfence_block()`)
ensures store-to-load ordering. Required between producer stores and consumer loads
when using ITS-based pipelining through shared memory.

---

## 3. Intra-Warp Divergence for Compute/Load Overlap

### 3.1 The Core Idea

The central question: **Can threads within the SAME warp intentionally diverge
so that quadpair 0-1 executes mma.sync while quadpair 2-3 issues LDG?**

**Answer: Partially, with significant caveats.**

The schedule optimizer CAN interleave execution of divergent code paths within
a warp. However, there are fundamental constraints:

1. **mma.sync creates implicit barriers** (see Section 5)
   - All 8 threads in a quadpair must arrive at the mma.sync instruction
   - This means the compute group (threads 0-15, quadpairs 0-1) must be
     synchronized at MMA boundaries
   - BUT the load group (threads 16-31, quadpairs 2-3) is free to execute
     independently during this time

2. **The schedule optimizer is not programmable**
   - You cannot force the hardware to schedule loads and MMA concurrently
   - You can only create conditions where the optimizer MAY choose to interleave
   - Divergent code paths + sufficient ILP + register availability guide the optimizer

3. **Load-compute overlap is more naturally achieved between warps**
   - Warp specialization (producer/consumer warps) is more predictable
   - The warp scheduler already interleaves instruction issue across 4 warp slots
   - ITS adds intra-warp flexibility on top of inter-warp scheduling

### 3.2 Practical Intra-Warp Pipeline Pattern

```cuda
// Conceptual -- within a single warp of 32 threads
const int qp_id = threadIdx.x / 8;  // quadpair 0-3

// Stage 0: Initial load
half2 k_frag[2];
load_k_tile_from_global(k_frag, K_ptr, tile_0);

for (int tile = 0; tile < num_tiles; tile++) {
    if (qp_id < 2) {
        // Quadpairs 0-1: Execute MMA on current tile
        // mma.sync internally synchronizes this quadpair
        mma_m8n8k4(d_frag, q_frag, k_frag, d_frag);
    } else {
        // Quadpairs 2-3: Prefetch next tile from global memory
        load_k_tile_from_global(k_next_frag, K_ptr, tile + 1);
    }

    __syncwarp();  // Reconverge entire warp

    // Broadcast loaded data from QP 2-3 to QP 0-1 via SMEM
    if (qp_id >= 2) {
        store_to_smem(k_next_frag, smem_buf);
    }
    __syncwarp();
    if (qp_id < 2) {
        k_frag = load_from_smem(smem_buf);
    }
    __syncwarp();
}
```

### 3.3 Why This is Problematic for FlashAttention

The pattern above has a **fundamental issue for FA**: the MMA compute path and
the load path need DIFFERENT fragments. In FA:

- **KQ phase**: Q fragments (in registers) x K fragments (from global/SMEM) = KQ scores
- **PV phase**: P fragments (softmax of KQ) x V fragments (from global/SMEM) = output

The producer quadpairs loading K/V data need to **communicate** the loaded data
to the consumer quadpairs doing MMA. This requires shared memory round-trips,
adding latency that may negate the benefit of overlap.

**The more effective pattern for FA is inter-warp specialization** (Section 3.4).

### 3.4 Inter-Warp Specialization (Recommended for Phase 4)

Instead of splitting a single warp, use dedicated warps for different roles:

```cuda
// Warp-level specialization with named barriers
const int warp_id = threadIdx.x / 32;

if (warp_id < NUM_COMPUTE_WARPS) {
    // Compute warps: execute MMA pipeline
    for (int tile = 0; tile < num_tiles; tile++) {
        // Wait for producers to fill SMEM buffer
        asm volatile("bar.sync 1, %0;" :: "r"(NUM_PRODUCER_THREADS));

        // Load from SMEM and compute
        load_fragments_from_smem(q_frag, k_frag, smem_buf[tile % 2]);
        mma_m8n8k4(d_frag, q_frag, k_frag, d_frag);

        // Signal consumption complete
        asm volatile("bar.sync 2, %0;" :: "r"(NUM_CONSUMER_THREADS));
    }
} else {
    // Producer warps: load data from global to SMEM
    for (int tile = 0; tile < num_tiles; tile++) {
        // Wait for consumers to finish with current buffer
        asm volatile("bar.sync 2, %0;" :: "r"(NUM_CONSUMER_THREADS));

        // Load next tile into double-buffered SMEM
        load_tile_global_to_smem(K_ptr, tile, smem_buf[tile % 2]);

        // Signal data ready
        asm volatile("bar.sync 1, %0;" :: "r"(NUM_PRODUCER_THREADS));
    }
}
```

This is the pattern used by CUTLASS 2.x for SM70 GEMM kernels (before cp.async
made it unnecessary on SM80+). It relies on the warp scheduler issuing
producer and consumer warps in interleaved fashion, which it does naturally.

**Advantages over intra-warp divergence:**
- No SMEM round-trip within a warp (producers write, consumers read -- one direction)
- Named barriers provide clean synchronization
- Warp scheduler naturally interleaves warps
- No reliance on unpredictable schedule optimizer behavior

**For Volta FA, this is the recommended approach.**

---

## 4. V100 Memory Latency and Pipeline Depth

### 4.1 Measured Latencies (Jia et al., arXiv:1804.06826)

| Memory Level | Latency (cycles) | Bandwidth |
|---|---|---|
| Register | 0 (same cycle) | N/A |
| Shared Memory (no conflict) | ~19 cycles | 14.8 TB/s (aggregate) |
| L1 Cache | ~28 cycles | ~116 GB/s per SM |
| L2 Cache | ~193 cycles | ~3.1 TB/s (aggregate) |
| HBM2 (DRAM) | ~300-350 cycles | 900 GB/s (peak) |

**Note on DRAM latency**: The Jia et al. paper measures L2 latency at 193 cycles.
Global memory (HBM2 DRAM) latency is the L2 miss penalty, which adds approximately
100-150 cycles on top of L2, giving a total **DRAM latency of approximately
300-350 cycles**. This is consistent with the broader GPU memory latency
literature (Ampere measured at ~290 cycles DRAM in similar microbenchmarks,
and Volta HBM2 has slightly higher latency than Ampere HBM2E).

**V100 clock**: 1.245-1.380 GHz (base-boost for SXM2). At 1.38 GHz:
- 300 cycles = ~217 ns
- 350 cycles = ~254 ns

### 4.2 Load Instruction Characteristics

**LDG.E.128** (128-bit global load):
- Loads 16 bytes per thread (e.g., 8 FP16 values or 4 FP32 values)
- A full warp issues 32 x 16B = 512B per load instruction
- Throughput: limited by LSU pipeline (1 load per cycle per warp, 32 LSUs per SM)
- Coalesced access pattern critical -- uncoalesced degrades to per-sector replay

**LDS.128** (128-bit shared memory load):
- 19-cycle latency (no bank conflict)
- 32 banks x 4B = 128B per cycle aggregate SMEM bandwidth
- Bank conflicts serialize: 2-way = 2 cycles, 32-way = 32 cycles
- XOR swizzle eliminates systematic bank conflicts

### 4.3 MMA Throughput

**mma.sync.aligned.m8n8k4.row.col.f32.f16.f16.f32:**
- 8 threads (quadpair) execute one m8n8k4 in 1 cycle (throughput, not latency)
- Output: 8x8 = 64 FP32 results (8 per thread)
- Arithmetic: 8 x 8 x 4 x 2 = 512 FP16 FLOPs per atom
- Per-SM throughput: 8 tensor cores x 64 FP16 FLOPs/cycle = 512 FP16 FLOPs/cycle
  (at 1-cycle throughput per tensor core)
- At 1.38 GHz: 512 x 1.38e9 = ~707 GFLOPS per SM (FP16 FMA)
- 80 SMs: ~56.5 TFLOPS (matches V100 spec: 56 TFLOPS FP16 not using tensor)

**For Volta HMMA (tensor core FP16):**
- Spec: 125 TFLOPS (V100 SXM2)
- Per SM: 125e12 / 80 / 1.38e9 = ~1131 FP16 FLOPs/cycle per SM
- This implies ~2 HMMA operations per cycle per tensor core on average

**Effective MMA latency**: The hardware MMA pipeline has latency of ~4 cycles
(same as dependent FMA on Volta). Throughput is 1 per cycle per tensor core
with sufficient ILP.

### 4.4 Pipeline Depth Calculation

**To hide 300-350 cycle DRAM latency with MMA throughput:**

Each m8n8k4 atom:
- Computes 512 FP16 FLOPs
- Takes 1 cycle throughput (4 cycles latency, but pipelined)
- Consumes: A matrix = 8x4 = 32 FP16 = 64 bytes, B matrix = 4x8 = 32 FP16 = 64 bytes
  Total: 128 bytes per atom

**Pipeline stages needed**:
- DRAM latency: ~320 cycles (midpoint estimate)
- MMA throughput: 1 atom/cycle/TC = 128 bytes consumed per cycle per TC
- Per-warp: 4 quadpairs = 4 MMA atoms in flight (if fully utilized)
- Time to compute one tile (D_chunk=32, K=4): 32/4 = 8 MMA atoms = 8 cycles
- **Stages to hide DRAM: ceil(320 / 8) = 40 stages** (!)

This is impractically large for register-buffered staging. **This is exactly why
double-buffering alone is insufficient on V100 and why multi-warp pipelining
is essential** -- you need enough warps in flight to collectively hide the
DRAM latency through the warp scheduler.

**Practical calculation with warp-level interleaving:**
- 4 warp scheduler slots per SM sub-partition
- Each slot can hold a different warp
- If all 4 slots have warps with loads in flight:
  Effective per-warp hiding = 320 / 4 = 80 cycles per warp
- 80 cycles / 8 cycles per tile = **10 tiles, approximately 3-4 pipeline stages** per warp

**Conclusion**: With 4 warps actively interleaved per sub-partition (Volta has
4 sub-partitions per SM, each with 1 warp scheduler), **3-4 double-buffered
SMEM stages** combined with the warp scheduler provides sufficient latency
hiding. This matches CUTLASS 2.x conventions for SM70.

---

## 5. mma.sync and ITS: The Synchronization Tension

### 5.1 What ".sync" Means

The `.sync` qualifier on `mma.sync.aligned.m8n8k4` means:

> **All threads in the executing group (quadpair) must have converged to the
> mma.sync instruction before it executes.**

This is an **implicit barrier within the quadpair** (8 threads). The hardware
will stall any thread that arrives at mma.sync until all 8 threads in its
quadpair are present.

### 5.2 Interaction with ITS

This creates a fundamental tension with ITS-based intra-warp pipelining:

**Scenario: Intentional intra-warp divergence**
```
Threads 0-15 (QP 0-1):  if (qp < 2) { mma.sync(...); }
Threads 16-31 (QP 2-3): else         { LDG.E.128(...); }
```

What happens:
1. Threads 0-7 (QP0) arrive at mma.sync -> execute (all 8 present) [OK]
2. Threads 8-15 (QP1) arrive at mma.sync -> execute (all 8 present) [OK]
3. Threads 16-23 (QP2) issue LDG -> proceed independently [OK]
4. Threads 24-31 (QP3) issue LDG -> proceed independently [OK]

**This works** because the `.sync` barrier is per-quadpair, not per-warp.
QP0 and QP1 synchronize internally for their respective MMA operations,
while QP2 and QP3 are free to execute loads concurrently.

### 5.3 The Real Constraint

The constraint is not that mma.sync prevents overlap -- it is that:

1. **All 8 threads in a QP must have their input registers ready**
   - A fragments (2 x uint32 = 4 FP16 values)
   - B fragments (2 x uint32 = 4 FP16 values)
   - C fragments (8 x float for FP32 accumulator variant)
   - If any thread in the QP is still loading its fragment, the entire QP stalls

2. **The schedule optimizer may not interleave as hoped**
   - The optimizer groups threads by PC, but it is an optimization, not a guarantee
   - In practice, if QP0-1 are doing MMA and QP2-3 are doing loads, the optimizer
     CAN schedule them concurrently... but it depends on resource availability
   - If the SM is fully occupied (many warps), the scheduler has other warps to
     choose from and may not need intra-warp interleaving

3. **Register pressure**
   - Each quadpair in the "compute" role needs: A(2) + B(2) + C(8) = 12 registers
     per MMA atom. For 4 atoms in a D_chunk=32 inner loop: ~48 registers
   - Each quadpair in the "load" role needs: destination registers for LDG
     (4-8 registers per 128-bit load)
   - All 32 threads share the same register allocation (compiler cannot allocate
     differently per branch)

### 5.4 Practical Implication for Phase 4

**Intra-warp QP-level divergence is technically possible but not the recommended
primary mechanism.** Use it as a secondary optimization on top of inter-warp
specialization:

1. **Primary**: Warp specialization (producer/consumer warps with named barriers)
2. **Secondary**: Within compute warps, the schedule optimizer may naturally
   overlap load-data-from-SMEM and MMA instructions even without explicit
   divergence -- Volta's ITS handles this transparently
3. **Tertiary**: If profiling shows bubbles, try explicit QP-level divergence
   as a micro-optimization

---

## 6. Published Examples and Prior Art

### 6.1 FlashAttention v1/v2 -- No V100 Implementation

**FlashAttention v1** (Dao et al., 2022, arXiv:2205.14135) and **v2** (2023,
arXiv:2307.08691) are built around:
- m16n8k16 / m16n8k8 MMA atoms (Ampere SM80+)
- cp.async for async global-to-SMEM copies
- Pipeline stages using mbarrier (Hopper) or cp.async commit groups (Ampere)

**V100 (SM70) is explicitly not supported.** The FlashAttention codebase states
SM80 minimum. No ITS-based pipelining is present because the target architectures
have hardware async instead.

### 6.2 FastAttention (arXiv:2410.16663) -- Layout Focus, Not ITS

FastAttention targets V100 and achieves 1.43x speedup over xformers. Its
contributions are:
- **Redesigned SMEM layout** for Volta's m8n8k4 quadpair structure
- **CPU-GPU cooperative strategy** for memory management (not ITS)
- Compile-time layout converter matching CuTe MMA_Traits for SM70

**FastAttention does NOT explicitly exploit ITS for software pipelining.**
Its speedup comes from eliminating layout conversion overhead and bank
conflicts -- which is exactly what our Phase 2 (XOR swizzle) and Phase 4
(inline PTX) target.

### 6.3 CUTLASS 2.x SM70 GEMM -- Double-Buffered Pipeline

CUTLASS 2.x supports SM70 with a **double-buffered software pipeline**:

```
// CUTLASS 2.x SM70 mainloop pattern (simplified)
// Two SMEM buffers: smem[0] and smem[1]

// Prologue: load first tile into smem[0]
global_to_smem(gmem, smem[0]);
__syncthreads();

for (int k = 0; k < K_tiles; k++) {
    // Load next tile into alternate buffer (overlapped with compute)
    global_to_smem(gmem + (k+1)*tile_size, smem[(k+1) % 2]);

    // Compute MMA on current buffer
    smem_to_reg(smem[k % 2], a_frag, b_frag);
    mma_sync(d_frag, a_frag, b_frag, d_frag);

    __syncthreads();  // Ensure load completes before buffer swap
}
```

**Key point**: CUTLASS 2.x for SM70 does NOT use explicit ITS or warp
specialization. It uses the **same warps** for both loading and computing,
with `__syncthreads()` as the pipelining barrier. The warp scheduler's
natural interleaving of warps at different pipeline stages provides latency
hiding.

CUTLASS 3.x added warp specialization for SM90 (Hopper) using TMA, but
the SM70 path remained with the simpler double-buffer approach.

### 6.4 Jaccard Weights Kernel (Besta et al., 2018) -- Actual ITS Exploitation

This is the **only published kernel we found that explicitly exploits ITS on Volta**
for performance. Key findings:

- 5x speedup on V100 compared to previous GPU generations
- Exploits ITS by allowing threads within a warp to traverse **different rows**
  of a sparse matrix simultaneously
- Data read by one thread may be reused by another thread on a different
  execution path -- ITS enables this interleaving
- The benefit is **enhanced data reuse** through interleaved divergent branches,
  not compute/load overlap

**Important distinction**: The Jaccard kernel is memory-bound and benefits from
ITS enabling better cache reuse through interleaved access patterns. Our FA
kernel is compute-bound in the MMA phase and memory-bound in the load phase --
the benefit model is different.

### 6.5 Twill (arXiv:2512.18134) -- Optimal SWP/WS Theory

This 2025 paper presents the first system for automatically deriving optimal
Software Pipelining (SWP) and Warp Specialization (WS) schedules. Key findings:

- Proves optimal schedules for FlashAttention on Hopper and Blackwell
- The theory is architecture-independent but the implementations target SM90+
- **Volta is mentioned as the origin of tensor cores** but no SM70 schedules
  are derived (because SM70 lacks the async primitives that SWP/WS exploit)
- Confirms that the **core challenge** is overlap of memory and compute --
  the same problem we are addressing with ITS

---

## 7. ITS Pitfalls and Deadlock Risks

### 7.1 __syncwarp Placement Errors

**Safe:**
```cuda
if (threadIdx.x < 16) {
    // ... compute ...
} else {
    // ... load ...
}
__syncwarp();  // All threads converge here -- safe
```

**Dangerous:**
```cuda
if (threadIdx.x < 16) {
    // ... compute ...
    __syncwarp();  // WRONG: only 16 threads arrive
} else {
    // ... load ...
    __syncwarp();  // These are different PCs on Volta!
}
```

On Volta with ITS, `__syncwarp()` at different PCs **may still work** because
there is only one warp-level barrier and diverged threads can converge at
different PCs. However, **this is not portable** and relies on implementation
details of Volta's barrier mechanism. Always converge before synchronizing.

### 7.2 __syncthreads in Divergent Code

**DEADLOCK on Volta:**
```cuda
if (threadIdx.x < blockDim.x / 2) {
    __syncthreads();  // Barrier instance A (at PC_1)
} else {
    __syncthreads();  // Barrier instance B (at PC_2)
    // Different PCs -> different barrier instances!
    // Each group waits for threads that will never arrive -> DEADLOCK
}
```

On Pascal, this accidentally worked because both `__syncthreads()` calls mapped
to the same barrier (same PC due to lockstep reconvergence). **Volta breaks
this** because threads at different PCs get different barrier instances (16
per CTA).

**Rule**: `__syncthreads()` must be reached by ALL threads in the block at
the same PC, or by no threads (thread has exited). For conditional synchronization,
use named barriers (`bar.sync N, count`).

### 7.3 Starvation and Progress

ITS can only provide progress guarantees for threads and warps that are
**resident** (scheduled on an SM). If forward progress depends on a thread
that hasn't been launched (because all SM slots are full), the system
cannot progress and will hang.

**Implication for our kernel**: Keep occupancy reasonable (10+ warps/SM is
fine at 190 regs/thread) and avoid inter-warp dependencies that require
ALL warps to make progress simultaneously.

### 7.4 Warp-Shuffle on Volta

Warp shuffle operations (`__shfl_sync`, `__shfl_down_sync`, etc.) require
the `_sync` variant on Volta (ITS-aware). The old non-sync variants
(`__shfl`, `__shfl_down`) are deprecated and may give wrong results because
threads may not be converged when the shuffle executes.

```cuda
// CORRECT on Volta:
float val = __shfl_sync(0xffffffff, my_val, src_lane);

// WRONG on Volta (works on Pascal by accident):
float val = __shfl(my_val, src_lane);  // May read stale/wrong value
```

---

## 8. Synthesis: ITS Pipeline Design for Volta FlashAttention

### 8.1 Recommended Architecture

Based on all findings, the recommended pipeline design for Phase 4 is:

**Level 1 -- Inter-warp double-buffering (primary latency hiding)**
```
Warps 0..N-1:  All warps participate in both load and compute
Buffer[0], Buffer[1]: Double-buffered SMEM tiles

Iteration i:
  1. Load K[i+1] from global -> SMEM buffer[(i+1)%2]
  2. Load K[i] fragments from SMEM buffer[i%2] -> registers
  3. Execute mma.sync on K[i] fragments
  4. __syncthreads()  -- ensure load completes before buffer swap
```

This is the CUTLASS 2.x SM70 pattern. The warp scheduler interleaves warps
at different stages of the pipeline, hiding DRAM latency.

**Level 2 -- Register-level prefetch within each warp**
```
Within a single warp's compute phase:
  frag_current = load from SMEM (current k-chunk)
  frag_next = load from SMEM (next k-chunk)      // prefetch
  mma.sync on frag_current
  frag_current = frag_next
  frag_next = load from SMEM (next+1 k-chunk)     // prefetch
  mma.sync on frag_current
  ...
```

This overlap of SMEM loads (19 cycles) with MMA execution (4-cycle latency,
1-cycle throughput) is handled naturally by the compiler and Volta's out-of-order
scheduling within a warp. ITS allows the schedule optimizer to interleave
SMEM load instructions with MMA instructions even within the same thread.

**Level 3 -- Occupancy-based hiding**
```
10 warps/SM at 190 regs/thread
4 sub-partitions per SM, each with 1 warp scheduler
~2-3 warps per sub-partition

Each scheduler interleaves 2-3 warps, hiding 2-3x of single-warp latency
```

### 8.2 What NOT to Do

1. **Do NOT rely on intra-warp QP-level divergence as the primary mechanism**
   - Unpredictable schedule optimizer behavior
   - Register pressure from uniform allocation across divergent paths
   - SMEM round-trips negate overlap benefit

2. **Do NOT use more than 2 SMEM pipeline stages** (for FA on V100)
   - SMEM is the critical resource (96 KB shared with L1)
   - 2 stages of K tiles + Q tiles + KQ output + VKQ output already
     consumes most of SMEM budget
   - 3+ stages would require reducing tile size, losing MMA efficiency

3. **Do NOT use nanosleep in the hot loop**
   - Adds unpredictable latency
   - Only useful for spin-waits in very asymmetric producer/consumer scenarios

### 8.3 Expected Benefit

The primary benefit of moving from WMMA to inline PTX m8n8k4 for our FA kernel
is NOT ITS-based software pipelining. It is:

1. **XOR swizzle** (2x SMEM throughput) -- requires manual address control
2. **Precise register allocation** -- WMMA's black-box allocation causes spilling at D=128
3. **FastAttention-style layout** -- native m8n8k4 data layout eliminates conversions
4. **Double-buffered SMEM** -- the standard pipeline pattern that WMMA prevents

ITS-based intra-warp pipelining is a **tertiary optimization** that may provide
an additional 5-15% on top of the above, primarily by allowing the schedule
optimizer to better interleave SMEM loads with MMA within each warp.

---

## 9. References

1. **Volta Architecture Whitepaper** (2017)
   https://images.nvidia.com/content/volta-architecture/pdf/volta-architecture-whitepaper.pdf

2. **Volta Tuning Guide** -- NVIDIA CUDA Toolkit Documentation
   https://docs.nvidia.com/cuda/volta-tuning-guide/index.html

3. **PTX ISA Documentation** -- NVIDIA
   https://docs.nvidia.com/cuda/parallel-thread-execution/index.html

4. **Hot Chips 2017: Volta** -- Jack Choquette, NVIDIA
   https://old.hotchips.org/wp-content/uploads/hc_archives/hc29/HC29.21-Monday-Pub/HC29.21.10-GPU-Gaming-Pub/HC29.21.132-Volta-Choquette-NVIDIA-Final3.pdf

5. **Dissecting the NVIDIA Volta GPU Architecture via Microbenchmarking** -- Jia et al. (2018)
   https://arxiv.org/abs/1804.06826

6. **Per-Thread Program Counters: A Tale of Two Registers** -- Farzad Khorasani
   https://medium.com/@farkhor/per-thread-program-counters-a-tale-of-two-registers-f2061949baf2

7. **A Jaccard Weights Kernel Leveraging Independent Thread Scheduling on GPUs** -- Besta et al. (2018)
   https://icl.utk.edu/files/publications/2018/icl-utk-1080-2018.pdf

8. **FastAttention: Extend FlashAttention2 to NPUs and Low-resource GPUs** -- arXiv:2410.16663
   https://arxiv.org/abs/2410.16663

9. **CUTLASS Tutorial: Efficient GEMM kernel designs with Pipelining** -- Colfax Research
   https://research.colfax-intl.com/cutlass-tutorial-design-of-a-gemm-kernel/

10. **Optimal Software Pipelining and Warp Specialization for Tensor Core GPUs** -- arXiv:2512.18134
    https://arxiv.org/abs/2512.18134

11. **CuTe MMA Atom Documentation** -- NVIDIA CUTLASS
    https://docs.nvidia.com/cutlass/media/docs/cpp/cute/0t_mma_atom.html

12. **CUTLASS SM70 MMA** -- NVIDIA/cutlass
    https://github.com/NVIDIA/cutlass/blob/main/include/cutlass/arch/mma_sm70.h

13. **CUTLASS CuTe MMA Traits SM70** -- NVIDIA/cutlass
    https://github.com/NVIDIA/cutlass/blob/main/include/cute/atom/mma_traits_sm70.hpp

14. **Nvidia Tensor Core -- Getting Started with MMA PTX Programming** -- Bruce-Lee-LY
    https://bruce-lee-ly.medium.com/nvidia-tensor-core-getting-started-with-mma-ptx-programming-508e44a6cb7d

15. **Automatic Kernel Generation for Volta Tensor Cores** -- arXiv:2006.12645
    https://arxiv.org/abs/2006.12645
