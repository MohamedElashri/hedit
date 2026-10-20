// Build: nvcc -g -G examples/cuda/vector_add.cu -o /tmp/hedit-vector-add
#include <cuda_runtime.h>
#include <cstdio>

__global__ void vector_add(const float* a, const float* b, float* out, int count) {
    const int i = blockIdx.x * blockDim.x + threadIdx.x;
    if (i < count) out[i] = a[i] + b[i];
}

int main() {
    constexpr int count = 256;
    float *a = nullptr, *b = nullptr, *out = nullptr;
    auto check = [](cudaError_t error) {
        if (error != cudaSuccess) std::fprintf(stderr, "CUDA: %s\n", cudaGetErrorString(error));
        return error == cudaSuccess;
    };
    if (!check(cudaMallocManaged(&a, count * sizeof(float)))) return 1;
    if (!check(cudaMallocManaged(&b, count * sizeof(float)))) { cudaFree(a); return 1; }
    if (!check(cudaMallocManaged(&out, count * sizeof(float)))) { cudaFree(b); cudaFree(a); return 1; }
    for (int i = 0; i < count; ++i) { a[i] = float(i); b[i] = 2.0f; }
    vector_add<<<2, 128>>>(a, b, out, count);
    bool ok = check(cudaGetLastError()) && check(cudaDeviceSynchronize());
    if (ok) {
        for (int i = 0; i < count; ++i) if (out[i] != float(i) + 2.0f) ok = false;
        std::printf("vector_add: %s\n", ok ? "PASS" : "FAIL");
    }
    cudaFree(out); cudaFree(b); cudaFree(a);
    return ok ? 0 : 1;
}
