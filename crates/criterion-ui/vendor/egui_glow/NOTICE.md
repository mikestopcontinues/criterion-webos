# egui_glow 0.36.2

Source: published `egui_glow` 0.36.2 crate (upstream revision is recorded in `.cargo_vcs_info.json`). The upstream MIT and Apache 2.0 licenses are retained. This local copy keeps the maintained painter and shader code while removing optional winit integration and its manifest dependencies; Criterion owns the platform window.

Local changes release a shader/program after compilation/link failure; release prior shaders/program/buffers after subsequent initialization failure; report missing shader locations and failed VAO creation as errors; and delete an allocated VAO during painter destruction. A callback type alias and removal of a workspace-specific lint expectation let this copy pass the project Clippy gate. No rendering algorithm or shader was replaced. Recheck these changes when updating the pinned upstream version.
