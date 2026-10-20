# CUDA and GPU tools

Use the **CUDA** button in the toolbar to open the GPU workbench. The editor
works without a GPU; the tools in this panel need the corresponding NVIDIA
software installed on Linux.

## Prepare a debug executable

Install a CUDA Toolkit and compatible NVIDIA driver. The workbench uses
`cuda-gdb`, `compute-sanitizer`, and `nvidia-smi` from `PATH`. You can enter a
custom CUDA-GDB path in the **Debugger** field.

Compile with host and device debugging information. For the included example,
run this from the source checkout or extracted package:

```sh
nvcc -g -G examples/cuda/vector_add.cu -o /tmp/hedit-vector-add
```

`-G` changes device optimization and performance, so use a separate debug build
when performance matters. NVIDIA describes the flags and supported hardware in
its [CUDA-GDB manual](https://docs.nvidia.com/cuda/cuda-gdb/index.html).

## Start and stop a debug session

1. Open the source file in hedit and enable **CUDA**.
2. Browse to the compiled **Executable**.
3. Enter program arguments as a JSON array of strings, or keep `[]`. For example,
   `["--size", "1024", "a path with spaces"]` passes three separate arguments.
4. Click **Start CUDA-GDB**. This loads the executable and adds a breakpoint at
   `main`; it does not run the program yet.
5. Enter `vector_add`, another function name, or `file.cu:42` in the breakpoint
   field and click **Add breakpoint**.
6. Click **Run**. Use **Continue** after the first stop to reach the next breakpoint.
7. Use **Stop** when finished. Closing the CUDA panel only hides it; it leaves the
   tool running until you stop it or quit hedit.

## Step and inspect

| Control | Use |
| --- | --- |
| Run | Start the loaded executable |
| Continue | Resume a paused program |
| Pause | Request an interruption |
| Step / Next / Finish | Step into, over, or out of a function |
| Stack | Show the current stack frames |
| Locals | Show local variables |
| Devices / Kernels / Threads / Warps | Inspect CUDA execution state |

Use the console for additional CUDA-GDB commands, such as:

```text
print i
info cuda blocks
cuda kernel 0
cuda thread (0,0,0)
info breakpoints
```

The selected CUDA focus determines what later commands inspect. Tool output is
selectable text, and the most recent 512 KiB is retained. Very fast output can
be truncated. Programs requiring interactive terminal input are not supported by
this panel.

The current workbench provides textual inspection. Source-gutter breakpoints,
a source-position indicator, structured watch trees, and lane visualizations are
not available yet.

## Run Compute Sanitizer

Stop any existing tool session first. Select the executable and its arguments,
choose a sanitizer, then click **Run sanitizer**. This immediately launches the
program under the selected checker.

- **memcheck**: memory-access errors.
- **racecheck**: shared-memory race hazards.
- **initcheck**: uninitialized device global-memory reads.
- **synccheck**: invalid synchronization usage.

Read the output and final process status. A detected error requests a nonzero
exit status. See NVIDIA's
[Compute Sanitizer guide](https://docs.nvidia.com/compute-sanitizer/ComputeSanitizer/index.html)
for tool-specific coverage.

## Check the GPU or resolve tool errors

**GPU info** runs `nvidia-smi` to display the device name, driver version, memory
usage, and temperature. It is a separate tool session.

A “could not start” message usually means the selected tool is missing or not on
`PATH`. A tool that starts and then fails may report driver, device, executable,
or debugger-permission errors in its output. Consult that output and the
[troubleshooting guide](troubleshooting.md).

NVIDIA hardware debugging has not been validated in this development environment.
The current automated checks cover the editor's process transport and error
handling; they do not certify a particular GPU or driver combination.
