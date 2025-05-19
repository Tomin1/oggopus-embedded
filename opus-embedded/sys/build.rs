/*
 * Copyright (c) 2025 Tomi Leppänen
 * SPDX-License-Identifier: BSD-3-Clause
 *
 * Builds minimal libopus for decoding with fixed point decoder and no dred.
 */

use bindgen::callbacks::ParseCallbacks;
use regex::Regex;
use std::env;
use std::ffi::OsString;
use std::fs::create_dir_all;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug)]
struct ParseCallback {
    cargo_callbacks: bindgen::CargoCallbacks,
    replacements: Vec<(Regex, &'static str)>,
}

impl ParseCallback {
    fn new() -> Self {
        ParseCallback {
            cargo_callbacks: bindgen::CargoCallbacks::new(),
            replacements: vec![
                (
                    Regex::new(r"\[(?<text>(in|out))\] ").unwrap(),
                    r"_\[$text\]_",
                ),
                (
                    Regex::new(r"\n(?<text>\* `error`[^\n]+)\n(?<after>[^\*]*$)").unwrap(),
                    "\n$text\n\n$after",
                ),
                (Regex::new(r"#(?<text>[A-Z_]+)").unwrap(), r"[`$text`]"),
                (
                    Regex::new(r"@retval #?(?<val>[A-Z_]+) (?<text>.*)").unwrap(),
                    "\n\n* [`$val`] $text",
                ),
                (Regex::new(r"@retval (?<text>.*)").unwrap(), "\n\n* $text"),
            ],
        }
    }
}

impl ParseCallbacks for ParseCallback {
    fn process_comment(&self, comment: &str) -> Option<String> {
        doxygen_bindgen::transform(comment)
            .map(|comment| {
                let mut comment = comment
                    .replace("[`opus_errorcodes`]", "opus error codes")
                    .replace("#OPUS_RESET_STATE", "`OPUS_RESET_STATE`")
                    .replace(
                        "@retval #OPUS_OK",
                        "\n\n\n# Returns\n\n@retval #OPUS_OK",
                    )
                    .replace(
                        "@retval OPUS_BANDWIDTH_NARROW",
                        "\n\n# Returns\n\n@retval OPUS_BANDWIDTH_NARROW",
                    )
                    .replace(
                        " # See also\n\n> [`opus_decoder_create,opus_decoder_get_size`]",
                        "\nSee also [`opus_decoder_create`] and [`opus_decoder_get_size`].",
                    );
                for (regex, replacement) in &self.replacements {
                    comment = regex.replace_all(&comment, *replacement).to_string();
                }
                comment
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
    // Make a copy of libopus to OUT_DIR so we can run autoreconf without modifying sources
    let target = PathBuf::from(env::var("OUT_DIR").unwrap()).join("opus");
    create_dir_all(&target).unwrap();
    let inputs: Vec<_> = std::fs::read_dir("src/opus")
        .unwrap()
        .map(Result::unwrap)
        .filter(|entry| entry.file_name().as_encoded_bytes()[0] != b'.')
        .map(|entry| entry.path().as_os_str().to_owned())
        .collect();
    let mut args = vec![OsString::from("-r"), OsString::from("--")];
    args.extend(inputs);
    args.push(OsString::from(&target));
    Command::new("cp").args(&args).status().unwrap();

    // Run autoreconf and configure in the new directory
    let mut builder = autotools::Config::new(target);
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
        let src_path = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("src");
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
