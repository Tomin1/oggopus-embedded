/*
 * Copyright (c) 2025 Tomi Leppänen
 * SPDX-License-Identifier: BSD-3-Clause
 *
 * Builds minimal libopus for decoding with fixed point decoder and no dred.
 */

use bindgen::callbacks::ParseCallbacks;
use std::env;
use std::path::PathBuf;

#[derive(Debug)]
struct ParseCallback {
    cargo_callbacks: bindgen::CargoCallbacks,
}

impl ParseCallback {
    fn new() -> Self {
        ParseCallback {
            cargo_callbacks: bindgen::CargoCallbacks::new(),
        }
    }
}

impl ParseCallbacks for ParseCallback {
    fn process_comment(&self, comment: &str) -> Option<String> {
        doxygen_bindgen::transform(comment)
            .map(|comment| {
                comment
                    .replace("[in] ", "_\\[in\\]_")
                    .replace("[out] ", "_\\[out\\]_")
                    .replace("[`opus_errorcodes`]", "opus error codes")
                    .replace(
                        "[`opus_decoder_create,opus_decoder_get_size`]",
                        "[`opus_decoder_create`], [`opus_decoder_get_size`]",
                    )
            })
            .inspect_err(|err| {
                println!("cargo:warning=Could not transform doxygen comment: {comment}\n{err}");
            })
            .ok()
    }

    fn header_file(&self, filename: &str) {
        self.cargo_callbacks.header_file(filename)
    }

    fn include_file(&self, filename: &str) {
        self.cargo_callbacks.include_file(filename)
    }

    fn read_env_var(&self, key: &str) {
        self.cargo_callbacks.read_env_var(key)
    }
}

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
    if env::var("TARGET").unwrap().starts_with("thumbv6m-") {
        // No assembly implementation without SMULL (32-bit multiply with 64-bit result)
        // instruction that does not exist on Cortex-{M0,M0+,M1} (thumbv6m).
        // However optimizations seem to do a reasonable job here.
        builder.disable("asm", None);
    }
    if env::var("TARGET").unwrap().starts_with("thumbv7m-") {
        // Fails on Cortex-M3 (thumbv7m), disable CPU detection on embedded
        builder.disable("rtcd", None);
    }
    if env::var("CARGO_CFG_TARGET_OS").unwrap() == "none" {
        builder
            .cflag("-D_FORTIFY_SOURCE=0")
            .cflag("-DOVERRIDE_celt_fatal")
            .cflag("-DCUSTOM_SUPPORT")
            .cflag(format!("-I{}", src_path.to_str().unwrap()))
            .ldflag("-nostdlib");
    }
    if cfg!(feature = "optimize_libopus") {
        builder.cflag("-O3");
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
        .allowlist_function("opus_packet_get_.*")
        .allowlist_function("opus_strerror")
        .allowlist_var("OPUS_OK")
        .allowlist_var("OPUS_BAD_ARG")
        .allowlist_var("OPUS_BUFFER_TOO_SMALL")
        .allowlist_var("OPUS_INTERNAL_ERROR")
        .allowlist_var("OPUS_INVALID_PACKET")
        .allowlist_var("OPUS_UNIMPLEMENTED")
        .allowlist_var("OPUS_INVALID_STATE")
        .allowlist_var("OPUS_ALLOC_FAIL")
        .allowlist_var("OPUS_BANDWIDTH_.*")
        .default_visibility(bindgen::FieldVisibilityKind::Private)
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
        .parse_callbacks(Box::new(ParseCallback::new()));
    if env::var("CARGO_CFG_TARGET_OS").unwrap() != "none" {
        builder = builder
            .allowlist_function("opus_decoder_create")
            .allowlist_function("opus_decoder_destroy");
    }
    if cfg!(feature = "stereo") {
        builder = builder.clang_arg("-DOPUS_EMBEDDED_SYS_STEREO");
    }
    let bindings = builder.generate().expect("Unable to generate bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("opus_decoder_gen.rs"))
        .expect("Couldn't write bindings!");
}
