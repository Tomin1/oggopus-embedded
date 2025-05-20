Opus decoder
============
This is safe abstractions over [libopus] for decoding Opus audio. The build
links [libopus] statically and is no_std and no_alloc on targets without std
library. See also [the sys crate](sys) for more information about the build.

[libopus]: https://github.com/xiph/opus

The build for ARM has flags set for Cortex-M0+. Other microcontrollers could be
supported better with some work.

Note that the code might not work on some platforms if Decoder size differs.
Please file issue tickets when you see size mismatches.

[![Crates.io][cratesio-version]][cratesio-link]
[![BSD-3-Clause licensed][cratesio-license]](COPYING)
[![docs.rs][docsrs-badge]][docsrs-link]

[cratesio-version]: https://img.shields.io/crates/v/opus-embedded
[cratesio-license]: https://img.shields.io/crates/l/opus-embedded
[cratesio-link]: https://crates.io/crates/opus-embedded
[docsrs-badge]: https://img.shields.io/docsrs/opus-embedded
[docsrs-link]: https://docs.rs/opus-embedded/latest/opus_embedded/

Features
--------
This crate has some features that can be enabled or disabled as needed.

* `optimize_libopus` enables optimizing [libopus] build even in debug builds.
  This is important for performance and is enabled by default.
* `stereo` enables constructing Decoder for stereo streams. This increases
  Decoder struct size by about 50 %. Not enabled by default.

Note that the optimizations are not applied to any Rust code, only the
underlying C-written [libopus] library which would perform very poorly without
any optimization. This feature allows to disable those optimizations.

Stereo decoding is not always desired in embedded systems. Enable it if you are
decoding streams that may contain more than one channel of audio (per stream).

License
-------
This crate is BSD licensed. See [COPYING](COPYING) for more information.
Dependency crates have their own licenses.
