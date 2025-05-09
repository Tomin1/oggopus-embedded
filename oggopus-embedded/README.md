Ogg and opus header parsing
===========================
This can parse Ogg files as specified by
[RFC3533](https://datatracker.ietf.org/doc/html/rfc3533) and
[RFC7845](https://datatracker.ietf.org/doc/html/rfc7845) but only if they
contain only Opus headers and data.

If you need a more complete Ogg parser, you should look elsewhere. There are
lots of other implementations.

Limitations
-----------
This code was created for my personal hobby project where I needed to store
some short Opus encoded audio on flash in an embedded system. It is not
intended as a general purpose Ogg parser or Opus player and you should not use
it with untrusted inputs. In particular streaming or seeking is not supported
and Ogg file cannot contain streams other than Opus.

Please do not make demands that this should support this or that feature, thank
you! If you need something and you can write code, you can also implement it
yourself.

Family 255 and Reserved Channel Mapping support
-----------------------------------------------
Family 255 and Reserved channel mapping table parsing support can be enabled
with `family255` feature. It is usually not needed for decoding mono or stereo
audio and it makes OpusHeader struct to take more space so it's not enabled by
default.

Missing features
----------------
The parser is missing a few features you might expect although it already has
more than what I actually needed myself.

- CRC checks.
- Seeking.
- Streaming data (e.g. from filesystem or network).
- Parsing of Opus comments header.
- Downmixing coefficients for Family 1 Channel Mapping down to stereo audio.

These could be implemented. Feel free to submit PRs if you happen to implement
something.

License
-------
This crate is BSD licensed. See COPYING for more information. Dependency crates
have their own licenses.
