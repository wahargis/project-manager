# Volta-Renaissance Literature Database

| Ref | Title | Key Findings | Relevant To |
|-----|-------|-------------|-------------|
| arXiv:2410.16663 | FastAttention (V100) | 1.43x vs xformers, m8n8k4 layout, 256K on 8xV100 | Phase 3-4 |
| arXiv:2603.05451 | FlashAttention v4 | Software exponentials via FMA, Blackwell-only but FMA technique portable | Phase 5 |
| arXiv:2503.01840 | EAGLE-3 | 5.58x on dense, 1.5-2.4x on MoE, not in llama.cpp yet | Phase 8 |
| arXiv:2507.18553 | GPTQ=Babai Lattice | GPTQ is Babais nearest plane algorithm, imports 40yr lattice theory | Cross-domain |
| arXiv:2506.05432 | PCDVQ | Direction 10x more sensitive than magnitude, E8 lattice codebook | Cross-domain |
| arXiv:2510.01240 | RSAVQ | Riemannian metric via Fisher Information for sensitivity-aware quant | Cross-domain |
| github:xlite-dev/ffpa-attn | FFPA Split-D | D-tiling at MMA level, SM80+ only, 1.8-3x vs SDPA | Phase 3 |
| github:ai-bond/flash-attention-v100 | FA-V100 Port | Proof of concept, D>64 known suboptimal, CUTLASS based | Phase 1 |
