# SM70 m8n8k4 MMA Output Layout

Reference for the NVIDIA V100 (SM70) `m8n8k4` HMMA tensor core instruction output
register layout. Decoded from the CuTe `SM70_8x8_32b` layout definition.

## CuTe Layout Definition

```cpp
// (T8,V8) -> (M8,N8)
using SM70_8x8_32b = Layout<Shape <Shape <_2, _2,_2>,Shape <_2,_2, _2>>,
                            Stride<Stride<_1,_16,_4>,Stride<_8,_2,_32>>>;
```

This maps 8 threads (T=0..7), each holding 8 float values (V=0..7), to an
8x8 output matrix C[M][N] where M=0..7 (rows) and N=0..7 (columns).

## Decoding

Thread index T decomposes as `T = t0 + 2*t1 + 4*t2` where t0,t1,t2 in {0,1}.
Value index V decomposes as `V = v0 + 2*v1 + 4*v2` where v0,v1,v2 in {0,1}.

The CuTe strides give:
```
flat_index = t0*1 + t1*16 + t2*4 + v0*8 + v1*2 + v2*32
```

Since the shape is (M8, N8) = (8, 8) in column-major order:
```
row = flat_index % 8
col = flat_index // 8
```

## Closed-Form Formulas

**Arithmetic** (verified exhaustively for all 64 entries):
```
row = t0 + 4*t2 + 2*v1
col = v0 + 2*t1 + 4*v2
```

Derivation: `row = flat_index % 8`. Since 16%8=0, 8%8=0, 32%8=0, only the
terms with stride < 8 survive: `t0*1 + t2*4 + v1*2`. For col, `flat_index = row + 8*col`,
so `col = (t1*16 + v0*8 + v2*32) / 8 = 2*t1 + v0 + 4*v2`.

**Bitwise** (verified exhaustively):
```
row = (tid & 1) | (tid & 4) | (vid & 2)
col = (vid & 1) | (tid & 2) | (vid & 4)
```

**Bit-level decomposition:**
| Output bit | Source    |
|------------|-----------|
| row[0]     | tid[0]    |
| row[1]     | vid[1]    |
| row[2]     | tid[2]    |
| col[0]     | vid[0]    |
| col[1]     | tid[1]    |
| col[2]     | vid[2]    |

The 6 input bits (3 from tid, 3 from vid) are interleaved across row and col.
This is a bit-shuffle, not a shift-and-mask pattern.

## 8x8 Matrix Ownership

Each cell shows `Thread.Value`:
```
         N=0    N=1    N=2    N=3    N=4    N=5    N=6    N=7
       +------+------+------+------+------+------+------+------+
M=0  |  0.0    0.1    2.0    2.1    0.4    0.5    2.4    2.5
M=1  |  1.0    1.1    3.0    3.1    1.4    1.5    3.4    3.5
M=2  |  0.2    0.3    2.2    2.3    0.6    0.7    2.6    2.7
M=3  |  1.2    1.3    3.2    3.3    1.6    1.7    3.6    3.7
M=4  |  4.0    4.1    6.0    6.1    4.4    4.5    6.4    6.5
M=5  |  5.0    5.1    7.0    7.1    5.4    5.5    7.4    7.5
M=6  |  4.2    4.3    6.2    6.3    4.6    4.7    6.6    6.7
M=7  |  5.2    5.3    7.2    7.3    5.6    5.7    7.6    7.7
```

**Structure:**
- **Top half** (M=0..3): threads 0-3. **Bottom half** (M=4..7): threads 4-7.
- Each thread owns 2 rows (stride-2 apart) and 4 columns.
- The matrix has a 4x4 block-of-2x2 structure: each 2x2 sub-block is owned by
  two consecutive threads (e.g., T0,T1 own the top-left 2x2 in each block).

## Per-Thread Register Map

```
Thread 0: V0->M[0][0]  V1->M[0][1]  V2->M[2][0]  V3->M[2][1]  V4->M[0][4]  V5->M[0][5]  V6->M[2][4]  V7->M[2][5]
Thread 1: V0->M[1][0]  V1->M[1][1]  V2->M[3][0]  V3->M[3][1]  V4->M[1][4]  V5->M[1][5]  V6->M[3][4]  V7->M[3][5]
Thread 2: V0->M[0][2]  V1->M[0][3]  V2->M[2][2]  V3->M[2][3]  V4->M[0][6]  V5->M[0][7]  V6->M[2][6]  V7->M[2][7]
Thread 3: V0->M[1][2]  V1->M[1][3]  V2->M[3][2]  V3->M[3][3]  V4->M[1][6]  V5->M[1][7]  V6->M[3][6]  V7->M[3][7]
Thread 4: V0->M[4][0]  V1->M[4][1]  V2->M[6][0]  V3->M[6][1]  V4->M[4][4]  V5->M[4][5]  V6->M[6][4]  V7->M[6][5]
Thread 5: V0->M[5][0]  V1->M[5][1]  V2->M[7][0]  V3->M[7][1]  V4->M[5][4]  V5->M[5][5]  V6->M[7][4]  V7->M[7][5]
Thread 6: V0->M[4][2]  V1->M[4][3]  V2->M[6][2]  V3->M[6][3]  V4->M[4][6]  V5->M[4][7]  V6->M[6][6]  V7->M[6][7]
Thread 7: V0->M[5][2]  V1->M[5][3]  V2->M[7][2]  V3->M[7][3]  V4->M[5][6]  V5->M[5][7]  V6->M[7][6]  V7->M[7][7]
```

## Shared Memory Store / Load

```cuda
// Store 8 MMA output registers from one thread to row-major smem float[8][8].
// tid: thread index within the 8-thread MMA group (0..7)
__device__ __forceinline__
void store_mma_m8n8k4_to_smem(int tid, const float* regs, float* smem) {
    const int t_row = (tid & 1) | (tid & 4);  // bits 0,2 of row from thread
    const int t_col = (tid & 2);               // bit 1 of col from thread

    #pragma unroll
    for (int vid = 0; vid < 8; vid++) {
        int row = t_row | (vid & 2);
        int col = (vid & 1) | t_col | (vid & 4);
        smem[row * 8 + col] = regs[vid];
    }
}

// Inverse: load from smem back to MMA register order
__device__ __forceinline__
void load_smem_to_mma_m8n8k4(int tid, const float* smem, float* regs) {
    const int t_row = (tid & 1) | (tid & 4);
    const int t_col = (tid & 2);

    #pragma unroll
    for (int vid = 0; vid < 8; vid++) {
        int row = t_row | (vid & 2);
        int col = (vid & 1) | t_col | (vid & 4);
        regs[vid] = smem[row * 8 + col];
    }
}
```

## Bank Conflict Analysis

V100 shared memory: 32 banks, 4-byte stride.
For row-major `float[8][8]`: `bank = (row * 8 + col) % 32`.

**Without swizzle: 2-way bank conflicts on every value index.**

The conflict is between threads that differ only in tid bit 2 (e.g., T0 and T4).
These threads access rows r and r+4. Since `(r+4)*8 - r*8 = 32`, they alias to the
same bank.

Example at V=0:
```
T0 -> (0,0) bank 0     T4 -> (4,0) bank 0    <-- conflict
T1 -> (1,0) bank 8     T5 -> (5,0) bank 8    <-- conflict
T2 -> (0,2) bank 2     T6 -> (4,2) bank 2    <-- conflict
T3 -> (1,2) bank 10    T7 -> (5,2) bank 10   <-- conflict
```

**With `col ^ row` swizzle: ZERO bank conflicts.**

```cuda
int swizzled_col = col ^ row;
smem[row * 8 + swizzled_col] = regs[vid];
```

Swizzled store function:
```cuda
__device__ __forceinline__
void store_mma_m8n8k4_to_smem_swizzled(int tid, const float* regs, float* smem) {
    const int t_row = (tid & 1) | (tid & 4);
    const int t_col = (tid & 2);

    #pragma unroll
    for (int vid = 0; vid < 8; vid++) {
        int row = t_row | (vid & 2);
        int col = (vid & 1) | t_col | (vid & 4);
        int swizzled_col = col ^ row;
        smem[row * 8 + swizzled_col] = regs[vid];
    }
}
```

Why it works: For conflicting threads T_lo (row r) and T_hi (row r+4), the
XOR makes their swizzled columns differ. The bank for T_lo is `(r*8 + c^r) % 32`
and for T_hi is `((r+4)*8 + c^(r+4)) % 32 = (r*8 + 32 + c^r^4) % 32 = (c^r^4) % 32`,
which differs from `(c^r) % 32` by a flip in bit 2 -- guaranteed different bank.

## Key Insight: Why This Layout?

The m8n8k4 MMA computes C[8x8] += A[8x4] * B[4x8] in one instruction. The
output layout interleaves thread and value bits so that:

1. **k-dimension accumulation is register-local**: When iterating over k (multiple
   m8n8k4 ops), each thread's 8 values accumulate in-place without cross-thread
   communication.

2. **Row continuity for A-matrix reuse**: Threads 0,1 (differing in tid bit 0) own
   adjacent rows. This means the same A-matrix row feeds both threads, enabling
   broadcast-efficient A loads.

3. **Column continuity for B-matrix reuse**: Threads 0,2 (differing in tid bit 1)
   own adjacent column pairs. Same B-matrix column feeds both.

## Generated By

Script: `/tmp/decode_sm70_layout.py`
Full output: `/tmp/sm70_mma_layout.txt`
