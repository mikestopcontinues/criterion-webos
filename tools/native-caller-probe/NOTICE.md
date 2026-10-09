# Native caller probe provenance

Application and protocol code are original Criterion Unofficial Rust, GPL-3.0-or-later. The SDL window seam is the existing `criterion-platform` crate; its owning [notice](../../crates/criterion-platform/NOTICE.md) supplies stock SDK and PlxNative attribution.

The LS2 declarations and compile-only ABI fixture follow LG's stock SDK `luna-service2/lunaservice.h` and the [maintained LS2 public interface](https://github.com/webosose/luna-service2/blob/master/include/public/luna-service2/lunaservice.h), copyright LG Electronics, Apache-2.0. They contain only the required public declarations and layout assertions. The SDK is unmodified; its license remains with the canonical development toolchain.

[PlxNative at `acca94a449a5f7db2c662d6501bd88199ce66d4e`](https://github.com/GLinnik21/plx-native/blob/acca94a449a5f7db2c662d6501bd88199ce66d4e/rust-modules/platform/src/keymanager.rs) was studied for native LS2 lifetime and Keymanager use. This probe does not copy its anonymous registration or storage fallback.
