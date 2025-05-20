libopus bindings for decoding
=============================
This is [libopus] bindings for decoding Opus. The build links [libopus]
statically and is no_std and no_alloc on targets without std library.

[libopus]: https://github.com/xiph/opus

The build for ARM has flags set for Cortex-M0+. Other microcontrollers could be
supported better with some work. Uses [libopus]'s autotools build system as
that seems to work well for cross compiling currently.

Note that the code might not work on some platforms if OpusDecoder size differs.
Please file issue tickets when you see size mismatches.

[![Crates.io][cratesio-version]][cratesio-link]
[![BSD-3-Clause licensed][cratesio-license]](COPYING)
[![docs.rs][docsrs-badge]][docsrs-link]

[cratesio-version]: https://img.shields.io/crates/v/opus-embedded-sys
[cratesio-license]: https://img.shields.io/crates/l/opus-embedded-sys
[cratesio-link]: https://crates.io/crates/opus-embedded-sys
[docsrs-badge]: https://img.shields.io/docsrs/opus-embedded-sys
[docsrs-link]: https://docs.rs/opus-embedded-sys/latest/opus_embedded_sys/

Features
--------
This crate has some features that can be enabled or disabled as needed.

* `optimize_libopus` enables optimizing [libopus] build even in debug builds.
  This is important for performance and is enabled by default.
* `stereo` makes OpusDecoder struct to take more space so that decoders for
  stereo streams can be initialized. Not enabled by default.

Abstractions over this crate should disable default features and include their
own respective features that enable these features case by case.

License
-------
This crate is BSD licensed just like [libopus]. See [COPYING](COPYING) and
[COPYING.libopus](COPYING.libopus) for more information. Dependency crates have
their own licenses.
