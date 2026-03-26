# Project Volta-Renaissance: Research Compendium
> **Project Thesis**: Volta-Renaissance is architectural specialization TO the NVIDIA V100 (SM70),
> not a port of newer techniques downward. Performance gains come FROM exploiting V100-specific
> features that newer architectures abandoned or changed. Every design decision asks:
> what does V100 do differently, and how can we exploit that?


**Goal**: Custom CUDA FlashAttention kernel for V100 (SM70) breaking D>64 barrier, targeting ~85 TFLOPS at D=128.
**GitHub**: HomeCloud #500
**Dev model**: Qwen3.5-0.8B (dense, D=128, same attention arch as production 35B-A3B)
**Prod model**: Qwen3.5-35B-A3B (MoE, same attention params)
**Hardware**: 5x V100-SXM2-32GB, 2 NVLink pairs

---

## 1. V100 FlashAttention State of Art

### 1.1 Existing Implementations
| Implementation | D=64 | D=128 | Notes |
|---|---|---|---|
| llama.cpp fattn-wmma | Working | Working (uncharacterized perf) | WMMA-based, our current path |
| ai-bond/flash-attention-v100 | Working | "Suboptimal" (author's words) | CUTLASS-based, proof-of-concept |
| FastAttention (arXiv:2410.16663) | 1.03-1.17x vs xformers | 1.43x with causal | Best published V100 FA; m8n8k4 layout optimizations |

### 1.2 V100 SM70 Hardware Constraints
- **Tensor core atom**: m8n8k4 (8x smaller K than Ampere's m16n8k16)
- **Registers**: 65,536/SM, 255 max/thread, 256-granularity/warp
- **SMEM**: 96KB configurable (up to 96KB L1 + 32KB SMEM or 48KB each)
- **No TMA, no cp.async** — all loads are synchronous through registers
- **No CUDA Graphs** on SM70 — full kernel launch overhead per step
- **HBM2**: 900 GB/s (vs A100 2039, H100 3350)

### 1.3 Key Technical Approaches

**Split-D Tiling** (novel for V100 — no existing implementation):
- Tile D=128 into D_chunk=32 or 64 micro-tiles
- Bounds registers to <=190/thread (zero spilling)
- FFPA (xlite-dev/ffpa-attn) does this on SM80+ but uses m16n8k16 atoms
- Must adapt to m8n8k4 — this is original work

**XOR Swizzled SMEM** (measured 2x improvement on Ampere):
- `swizzled_col = (row ^ col_chunk) * ELEMS_PER_BANK + col_offset`
- CuTe: `Swizzle<3,0,3>` for 8x8 FP16 tiles
- Eliminates 32-way bank conflicts
- No V100 FA kernel has this yet

**Software Exponentials** (from FA-4, portable):
- Cody-Waite range reduction + polynomial FMA evaluation
- Bypasses MUFU serialization
- On V100: MUFU is LESS constrained than SM90+ (not the primary bottleneck)
- Worth implementing if MUFU becomes bottleneck after other optimizations

**FastAttention m8n8k4 Layout** (arXiv:2410.16663):
- Redesigned SRAM layout for Volta's quadpair MMA
- Each thread produces 8 elements (vs 4 on Ampere)
- Compile-time layout converter from CuTe MMA_Traits
- 1.46x end-to-end for PanGu-38B on 8x V100

### 1.4 Occupancy at 190 regs/thread
- 190 regs x 32 threads = 6080/warp, rounded to 6144 (256 granularity)
- 65536 / 6144 = **10 warps/SM = 15.6% occupancy**
- Volta tuning guide: "4 warps sufficient for math latency hiding"
- 10 warps > 4 — viable for compute-bound phases
- KV-load phases need pipeline overlap to compensate

---

## 2. Graphics Compression -> LLM Inference Catalog

### 2.1 Already Converged (independently rediscovered)
| Graphics Technique | Year | LLM Equivalent | Year | Gap |
|---|---|---|---|---|
| BFN (direction > magnitude) | 2010 | PCDVQ direction sensitivity | 2025 | 15 years |
| BC1 block quantization | 1999 | GPTQ group quantization | 2022 | 23 years |
| E8 lattice (math) | 1965 | QuIP# E8 codebook | 2024 | 59 years |
| QTangent rotation encoding | 2011 | TurboQuant/QuIP# Hadamard | 2024 | 13 years |

### 2.2 Unexploited Opportunities
1. **BC7 partition search heuristics** — 64 precomputed patterns for mixed-precision per-block assignment. LLM mixed-precision uses expensive sensitivity analysis.
2. **Multi-mode blocks** — BC7 selects from 8 modes per 4x4 block. LLM quantization is one scheme globally. Per-block mode selection unexplored.
3. **P-bits** — BC7's shared LSB for fine endpoint positioning. In LLM: shared fine-adjustment bit across a weight group.
4. **Octahedral projection (L1 normalization)** — high-d analog not studied for KV cache preprocessing.
5. **Hardware texture decode for weight storage** — BC7-formatted weights get free decompression from texture units.

### 2.3 Key Cross-Domain Papers
- **GPTQ = Babai's nearest plane algorithm** (arXiv:2507.18553, NeurIPS 2025) — imports 40 years of lattice theory
- **RSAVQ** (arXiv:2510.01240) — Riemannian metric on weight space via Fisher Information
- **PCDVQ** (arXiv:2506.05432) — direction 10x more sensitive than magnitude, E8 lattice codebook
- **DartQuant** (arXiv:2511.04063) — rotation calibration, 47x faster than end-to-end rotation optimization

---

## 3. Speculative Decoding

### 3.1 What Works Today
Standard draft-model spec decode on ik_llama.cpp:
- `--model-draft Qwen3.5-0.8B --draft-max N --ctx-checkpoints`
- Projected 1.3-1.5x speedup (97 -> 126-146 tok/s)
- MoE models see reduced benefit vs dense (expert loading bottleneck)

### 3.2 What Doesn't (Yet)
- EAGLE-3: Not in llama.cpp/ik_llama.cpp, no pre-trained head for Qwen3.5-35B-A3B
- Qwen3.5 MTP heads: Only in vLLM
- MoE-Spec expert budgeting: Research-only

---

## 4. Project Phases

### Phase 1: Environment + Baseline Profiling (1-2 weeks)
- Set up CUTLASS 2.x/3.x build environment targeting SM70
- Profile existing fattn-wmma kernel on 0.8B model with Nsight Compute
- Measure: register usage, SMEM occupancy, bank conflicts, MUFU utilization
- Establish baseline TFLOPS at D=64 and D=128
- Validate theoretical register/SMEM budgets against ptxas output

### Phase 2: XOR Swizzle Integration (1 week)
- Add XOR swizzle to fattn-wmma SMEM layout
- Measure bank conflict elimination (expect ~2x SMEM throughput)
- This is the lowest-risk, highest-ROI optimization

### Phase 3: Split-D Prototype (2-3 weeks)
- Implement D-dimension tiling for m8n8k4 atoms
- Target: D=128 in 2 chunks of D_chunk=64, or 4 chunks of D_chunk=32
- Register budget: <=190/thread, zero spilling
- Standalone matmul kernel first, then integrate into FA forward pass
- Profile with Nsight Compute against non-tiled baseline

### Phase 4: FastAttention Layout Optimization (1-2 weeks)
- Port FastAttention's m8n8k4 layout converter
- Integrate with Split-D tiling from Phase 3
- Multi-stage software pipeline (3-4 stages)
- Target: >=60 TFLOPS at D=128

### Phase 5: Software Exponentials (1 week, conditional)
- Only if MUFU is measured as bottleneck in Phase 4
- Implement Cody-Waite + degree-3 polynomial
- FP32 FMA evaluation concurrent with HMMA

### Phase 6: NVLink Ring Attention (2 weeks)
- Sequence-parallel partitioning across NVLink pair
- cudaMemcpyPeerAsync overlapped with local compute
- Extended context benchmarks (128K, 256K)

### Phase 7: Integration + Production Validation (1-2 weeks)
- ik_llama.cpp kernel integration
- 0.8B correctness validation
- 35B-A3B production benchmarks
- Comparison: before/after on production workloads

### Phase 8: Speculative Decoding (parallel track)
- Standard draft-model with Qwen3.5-0.8B
- ctx-checkpoints for hybrid model support
- Benchmark on production 35B-A3B

---

## 5. Success Criteria

| Metric | Current | Target | Method |
|---|---|---|---|
| D=128 TFLOPS | ~18 (estimated) | >=60 | Nsight Compute |
| D=128 TG tok/s (0.8B) | ~95 | >=120 | llama-bench |
| D=128 TG tok/s (35B) | ~97 | >=110 | llama-bench |
| Bank conflicts | Unknown | 0 excess wavefronts | Nsight SMEM replay |
| Register spilling | Unknown (likely >0 at D=128) | 0 | ptxas --verbose |
| Max context (35B, 2xV100) | 524K (current) | 1M+ with ring attention | llama-bench -pg |

---

## 6. References

### V100 Architecture
- [Volta Tuning Guide](https://docs.nvidia.com/cuda/volta-tuning-guide/)
- [V100 Architecture Whitepaper](https://images.nvidia.com/content/volta-architecture/pdf/volta-architecture-whitepaper.pdf)
- [Tensor Core Evolution (SemiAnalysis)](https://newsletter.semianalysis.com/p/nvidia-tensor-core-evolution-from-volta-to-blackwell)

### FlashAttention
- [FA-1 (arXiv:2205.14135)](https://arxiv.org/abs/2205.14135)
- [FA-2 (arXiv:2307.08691)](https://arxiv.org/abs/2307.08691)
- [FA-3 (arXiv:2407.08608)](https://arxiv.org/abs/2407.08608)
- [FA-4 (arXiv:2603.05451)](https://arxiv.org/abs/2603.05451)
- [FastAttention V100 (arXiv:2410.16663)](https://arxiv.org/abs/2410.16663)
- [FFPA Split-D (xlite-dev/ffpa-attn)](https://github.com/xlite-dev/ffpa-attn)
- [ai-bond/flash-attention-v100](https://github.com/ai-bond/flash-attention-v100)

### Graphics Compression
- [GPTQ = Babai's Algorithm (arXiv:2507.18553)](https://arxiv.org/abs/2507.18553)
- [PCDVQ (arXiv:2506.05432)](https://arxiv.org/abs/2506.05432)
- [RSAVQ (arXiv:2510.01240)](https://arxiv.org/abs/2510.01240)
- [QuIP# E8 Lattice (arXiv:2402.04396)](https://arxiv.org/abs/2402.04396)
- [BC7 Format (Nathan Reed)](https://www.reedbeta.com/blog/understanding-bcn-texture-compression-formats/)
- [Octahedral Encoding (Knarkowicz)](https://knarkowicz.wordpress.com/2014/04/16/octahedron-normal-vector-encoding/)

### Speculative Decoding
- [EAGLE-3 (arXiv:2503.01840)](https://arxiv.org/abs/2503.01840)
- [EAGLE GitHub](https://github.com/SafeAILab/EAGLE)
- [ik_llama.cpp ctx-checkpoints (PR #1310)](https://github.com/ikawrakow/ik_llama.cpp/pull/1310)

### CuTe/CUTLASS
- [CuTe MMA Atoms](https://github.com/NVIDIA/cutlass/blob/main/media/docs/cpp/cute/0t_mma_atom.md)
- [CuTe Swizzle (Lei Mao)](https://leimao.github.io/blog/CuTe-Swizzle/)
- [SMEM Bank Conflicts + Swizzling (lubits.ch)](https://lubits.ch/flash/Part-4)
