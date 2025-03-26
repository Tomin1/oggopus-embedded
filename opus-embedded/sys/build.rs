/*
 * Copyright (c) 2025 Tomi Leppänen
 *
 * Builds minimal libopus for decoding with fixed point decoder and no dred.
 */

use std::env;
use std::path::PathBuf;

fn main() {
    let src_path = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("src");
    let mut builder = autotools::Config::new("src/opus");
    builder
        .reconf("-ivf")
        .disable("deep-plc", None)
        .disable("doc", None)
        .disable("dred", None)
        .disable("extra-programs", None)
        .disable("float-api", None)
        .enable("fixed-point", None);
    if env::var("CARGO_CFG_TARGET_ARCH").unwrap() == "arm" {
        // TODO: Should be "thumbv6m"
        // No assembly implementation without SMULL (32-bit multiply with 64-bit result)
        // instruction that does not exist on Cortex-{M0,M0+,M1} (thumbv6m)
        builder.disable("asm", None);
        // Fails on Cortex-M3 (thumbv7m), disable CPU detection on embedded
        builder.disable("rtcd", None);
    }
    if env::var("CARGO_CFG_TARGET_OS").unwrap() == "none" {
        builder
            .cflag("-DCUSTOM_SUPPORT")
            .cflag(format!("-I{}", src_path.to_str().unwrap()))
            .ldflag("-nostdlib");
    }
    let dst = builder.build();
    println!(
        "cargo:rustc-link-search=native={}",
        dst.join("lib").display()
    );
    println!("cargo:rustc-link-lib=static=opus");

    let mut builder = bindgen::Builder::default()
        .header("src/decoder.h")
        .allowlist_type("OpusDecoder")
        .allowlist_function("opus_decode")
        .allowlist_function("opus_decoder_get_nb_samples")
        .allowlist_function("opus_decoder_get_size")
        .allowlist_function("opus_decoder_init")
        .allowlist_function("opus_strerror")
        .allowlist_var("OPUS_OK")
        .allowlist_var("OPUS_BAD_ARG")
        .allowlist_var("OPUS_BUFFER_TOO_SMALL")
        .allowlist_var("OPUS_INTERNAL_ERROR")
        .allowlist_var("OPUS_INVALID_PACKET")
        .allowlist_var("OPUS_UNIMPLEMENTED")
        .allowlist_var("OPUS_INVALID_STATE")
        .allowlist_var("OPUS_ALLOC_FAIL")
        .default_visibility(bindgen::FieldVisibilityKind::Private)
        .generate_comments(false)
        .use_core()
        .clang_arg("-DDISABLE_DEBUG_FLOAT=1")
        .clang_arg("-DDISABLE_FLOAT_API=1")
        .clang_arg("-DFIXED_POINT=1")
        .clang_arg("-DFLOAT_APPROX=1")
        .clang_arg("-Isrc/opus/celt")
        .clang_arg("-Isrc/opus/dnn")
        .clang_arg("-Isrc/opus/include")
        .clang_arg("-Isrc/opus/silk")
        .derive_default(true)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()));
    if env::var("CARGO_CFG_TARGET_OS").unwrap() != "none" {
        builder = builder
            .allowlist_function("opus_decoder_create")
            .allowlist_function("opus_decoder_destroy");
    }
    let bindings = builder.generate().expect("Unable to generate bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("opus_decoder_gen.rs"))
        .expect("Couldn't write bindings!");
}
