Horizon #6447 - Radeon vertex-fetch probe v2
==========================================

Requires Windows, Python 3.10+ and OpenGL 4.3. No pip packages.
Run on the affected Radeon, WITHOUT RenderDoc:

    py -3 vertex_fetch_probe_v2.py --output results-native-v2.json

If your Python command is python, use:

    python vertex_fetch_probe_v2.py --output results-native-v2.json

Check adapter.renderer in the output identifies the affected Radeon. Attach
results-native-v2.json to https://github.com/horizon-rust/horizon/issues/6447
(rename a copy to .txt if GitHub rejects .json), plus terminal errors if any.
The script creates a hidden window, runs small GPU tests and writes JSON. It
does not access game files, accounts, saves, launcher settings or the network.

What the first Radeon report actually showed
-------------------------------------------
The aggregate FAIL lines hid partial successes. All UV values were correct.
The scalar u16 layer returned zero; u16x2 and packed u32 layer fetches returned
all seven expected values correctly. The separate u8x2 light returned zero
in every case. Both binding APIs and both draw variants showed this pattern.
The old negative control used another scalar u16 fetch and expected the
adjacent light, so its failure does not independently mean readback is broken.

This suggests, but does not prove, a problem with narrow integer fetches.
Widening the layer alone does not fix the observed light failure.

What v2 adds
------------
- Reports each field (layer, UV-u, UV-v, light) separately.
- Reads back uploaded input bytes and queries vertex attribute configuration.
- Reads layer+light together from their unchanged four bytes at offset16:
  u16x2, u32 (low16=layer, high16=light), and u8x4.
- Tests separate light input as u8x4, scalar u16 and u16x2 at offset18.
- Compares the original split transform-feedback outputs with one uvec4.
- Bypasses transform feedback for an independent RGBA32UI framebuffer and
  glReadPixels path, for the original layout and shared u32 candidate.
- Constant-output controls have no vertex inputs and test both readback paths.
- Swaps UV/layer shader locations and compares base vertex0 with37808.
- Uses shared u32 at a deliberately wrong offset12 as a negative control.
  It should decode UV bytes as layer/light. Failure is recorded as inconclusive
  and does not prevent other tests from finishing.

The matrix has 59 positive cases (including three constant controls) and four
negative controls. Every case passed on NVIDIA RTX3090/OpenGL4.3; all four
wrong-offset controls were detected. This is not validation on legacy AMD.

Reading the report
------------------
Each row's fields dictionary distinguishes layer failure from light failure.
The row's passed flag includes input-buffer and primitive-count checks.
constant=true rows must pass before relying on that output path. Check
input_buffer_readback_ok, primitive counts, expected/actual and API errors.
Pixel rows use no transform feedback and report primitives_written=null.

If original layer/light fail in pixels too, but shared u32 passes both TF and
pixels, that supports testing a four-byte shared-fetch workaround in Horizon.
If only TF fails, investigate capture/export instead of assuming vertex fetch.
If failure follows attribute location or base vertex rather than format, that
points to a different driver/state path. Mixed or failed controls need more
investigation; do not infer a confirmed fix from overall PASS/FAIL alone.

Exit0: all positive cases and negative controls passed.
Exit1: a value/validation mismatch or inconclusive negative control was recorded.
Exit2: a context/setup/API/probe error was recorded; later tests are attempted
where possible. Return the JSON even if the program exits nonzero.

Limits
------
This tests raw OpenGL vertex fetching and integer pixels with small point draws,
not Horizon's full world/texture-rendering pipeline. It does not independently
verify a later RenderDoc replay or prove the hardware driver is at fault.
No Horizon production code is changed by this probe.
