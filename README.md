Oggopus embdedded
=================
This is parser for Ogg files containing Opus audio with strong focus on
embedded systems, and also bindings for libopus Opus decoder. Those can be used
independently, and they are both no-std and no-alloc compatible.

Note that this code was created for my personal hobby project where I needed to
store some short Opus encoded audio on flash in an embedded system. It is not
intended as a general purpose Ogg parser or Opus player and you should not use
it with untrusted inputs. In particular streaming or seeking is not supported
and Ogg file cannot contain streams other than Opus. Please do not make demands
that this should support this or that feature, thank you! If you need something
and you can write code, you can also implement it yourself.

See the various COPYING files for license information.

Ogg and opus header parsing
---------------------------
See oggopus-embeddbed directory for Ogg parsing implementation. It can parse
Ogg files as specified by RFC3533 and RFC7845 but only if they contain only
Opus headers and data.

If you need a more complete Ogg parser, you should look elsewhere. There are
lots of other implementations.

### Missing features
The parser is missing a few features you might expect although it already has
more than what I actually needed myself.

- CRC checks.
- Downmixing coefficients for Family 1 Channel Mapping down to stereo audio.
- Parsing of Opus comments header.
- Seeking.
- Streaming data (e.g. from filesystem or network).

Opus decoder
------------
See opus-embedded directory for libopus bindings for decoding Opus. The build
links libopus statically and is no-std on targets without std library.

The build for ARM has flags set for Cortex-M0+. Other microcontrollers could be
supported better with some work.

Note that the code might not work on some platforms if OpusDecoder size differs.
Please file issue tickets when you see size mismatches.

Example player
--------------
There is a small example player in example-linux directory. It is dumbed down
to a fault but it can demonstrate that the libraries work.
