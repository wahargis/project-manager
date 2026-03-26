# Volta-Renaissance Decision Log

### [2026-03-26 01:26] Use 0.8B dense as dev model
**Why:** Same D=128 attention architecture as 35B-A3B, fast iteration, single GPU
**Alternatives considered:** 9B OmnicoderV2, 27B dense
**Phase:** Phase 1

### [2026-03-26 01:26] WMMA to inline PTX migration in Phase 4 not Phase 2
**Why:** XOR swizzle can be done within WMMA framework, lower risk first
**Alternatives considered:** Rewrite everything at once
**Phase:** Phase 2-4

### [2026-03-26 01:26] XOR swizzle before Split-D
**Why:** 2x SMEM throughput is low-risk high-ROI, does not change algorithm
**Alternatives considered:** Split-D first (higher risk)
**Phase:** Phase 2-3

### [2026-03-26 01:43] ALL algorithms must pass SM70 compatibility checklist
**Why:** Previous session ported SM80+ techniques without checking V100 compatibility, causing 3 GPU wedges. Every optimization must be validated against: m8n8k4 atoms, no TMA/cp.async, 255 reg limit, 96KB SMEM, synchronous loads only, no CUDA graphs.
**Alternatives considered:** Ad-hoc compatibility checking per technique
**Phase:** ALL phases

### [2026-03-26 01:45] Project thesis: exploit V100-specific architecture, not port SM80+ down
**Why:** V100 has unique features (ITS, quadpair MMA, unified L1/SMEM) that newer architectures changed. Performance comes FROM specialization. Every design asks: what does V100 do differently and how to exploit it.
**Alternatives considered:** Generic approach that works on any GPU
**Phase:** ALL phases — this is the project identity

### [2026-03-26 02:30] ITS is tertiary, focus Phase 4 on swizzle+registers+layout+double-buffer
**Why:** ITS pipelining adds 5-15% but primary wins come from swizzle (2x SMEM), register control (eliminate spilling), m8n8k4 layout (eliminate conversion), double-buffer (hide latency). Published V100 kernels dont use ITS pipelining — CUTLASS 2.x SM70 uses standard double-buffer.
**Alternatives considered:** Prioritize ITS as primary mechanism
**Phase:** Phase 4

### [2026-03-26 09:53] Create new flash_attn_ext_f16_ptx alongside existing WMMA kernel
**Why:** Modifying the existing kernel risks regression on the working D=64 path. A new kernel function can be dispatched for D>=128 only, keeping WMMA for D<=64. Side-by-side benchmarking possible.
**Alternatives considered:** Modify existing kernel in-place (smaller diff but higher risk)
**Phase:** Phase 4 Step 4

### [2026-03-26 10:04] Phase 4 Step 4: Separate PTX kernel file instead of mixed WMMA/PTX
**Why:** Mixing inline PTX with WMMA fragments in the same kernel is impossible — WMMA uses its own internal register layout that is incompatible with raw m8n8k4 fragment registers. Need a clean-sheet PTX kernel that handles Q load, K load, MMA, softmax, VKQ accumulation all in PTX. This is a larger effort but produces a truly V100-optimized kernel.
**Alternatives considered:** Mixed WMMA/PTX hybrid (impossible due to register layout mismatch)
**Phase:** Phase 4 Step 4

### [2026-03-26 10:32] SM70 m8n8k4 produces ONE 8x8 output per MMA call across all 32 threads. 4 QuadPairs are REDUNDANT copies, NOT independent compute units. For 32x8 (matching WMMA m32n8k16), need 4 separate MMA calls. Fragment loading: each thread provides one full row of A (a0=cols0-1, a1=cols2-3) and one full column of B (b0=rows0-1, b1=rows2-3), indexed by qp_ltid. Output stored via mma_out_coords(ltid,v)→(row,col) from QP0 only. Validated: 0/256 mismatches at D=128.
**Why:** 
**Alternatives considered:** 
**Phase:** 

