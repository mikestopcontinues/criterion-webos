# Decoder source

This directory contains the source, normalized Cargo manifest, README and complete MIT/Apache-2.0 licenses from the published [image-webp 0.2.4 package](https://crates.io/crates/image-webp/0.2.4). The upstream package SHA-256 is `525e9ff3e1a4be2fbea1fdf0e98686a6d98b4d8f937e1bf7402245af1909e8c3`.

The sole functional correction is in `src/huffman.rs`: canonical-code accumulation and complete-tree validation use `u32`, with checked conversion to the existing `u16` codes. This rejects an oversubscribed maximum-depth tree before decoding-table construction. Two trailing spaces in upstream `src/yuv.rs` test data are removed for repository whitespace checks. Upstream source and licensing remain intact otherwise. The application's public decode regression exercises this rejection and valid maximum-depth support.

The root Cargo manifest selects this directory through `[patch.crates-io]` and excludes its upstream development harness from workspace members. Application decoding uses the pure Rust library; upstream optional development comparisons against native WebP are not built by the application's checks.
