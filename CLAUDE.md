# Scout-Rust development caveats

Non-obvious constraints worth knowing when working in this codebase. Grouped by
subsystem. Keep entries short: one caveat, one reason, one pointer to code.

## Providers

### Runway (`gen4_image` / `gen4.5` / `gen4_aleph`)

- **`promptText` max length is 1000 UTF-16 code units** on `/text_to_image`
  (empirically enforced; the video endpoints are almost certainly the same).
  Over the limit Runway returns `400 Validation of body failed; promptText:
  Too big: expected string to have <=1000 characters`. When wrapping a
  per-scene prompt with a style anchor + constraints + quality-floor suffix,
  size-budget the variable portion and trim on a UTF-8 char boundary before
  appending the fixed parts. See `truncate_to_byte_boundary`,
  `FLASH_PROMPT_MAX_BYTES`, and the wrap in `run_flash_generation` in
  [src/jobs/worker.rs](src/jobs/worker.rs).
- **`ratio` is a strict enum** on image/video endpoints — Runway only accepts
  exact strings from its allowed list (e.g. `"1080:1920"`, `"1920:1080"`,
  `"1024:1024"`, `"1280:720"`, …). Arbitrary dimensions like `"1088:1920"`
  are rejected. Keep the mapping `format!("{width}:{height}")` in
  [src/providers/runway.rs](src/providers/runway.rs) tied to the exact values
  the caller passes.

### BFL Flux (`flux-2-max`)

- **`width` and `height` must be multiples of 32** in the range `[256, 2048]`.
  Passing `1080×1920` (common "9:16" dims) silently gets rounded by BFL,
  producing an image whose aspect ratio drifts off 9:16 — noticeable when a
  strictly-sized frontend container clips the edges. Snap to exact-aspect
  Flux-safe pairs (9:16 → `1152×2048`, 16:9 → `2048×1152`, 3:4 → `1536×2048`,
  4:3 → `2048×1536`, 1:1 → `1024×1024`) or round each axis to the nearest 32.
  See `flux_safe_dims` in [src/providers/flux.rs](src/providers/flux.rs).
