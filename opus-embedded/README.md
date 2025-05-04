Opus decoder
============
This is libopus bindings for decoding Opus. The build links libopus statically
and is no-std on targets without std library.

The build for ARM has flags set for Cortex-M0+. Other microcontrollers could be
supported better with some work.

Note that the code might not work on some platforms if OpusDecoder size differs.
Please file issue tickets when you see size mismatches.
