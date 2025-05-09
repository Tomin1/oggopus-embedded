Example player
==============
A small example player. It is dumbed down to a fault but it serves as a test
and an example of usage of oggopus-embedded and opus-embedded crates.

This example only works on Linux since it requires ALSA.

Usage
-----
Build and run:
```sh
cargo run --release -- ~/Music/My\ Favourite\ Song.opus
```

The opus file must use channel mapping family 0 which is usually the default
when creating mono or stereo audio file.

License
-------
This example is BSD licensed. See COPYING for more information. Dependency
crates have their own licenses.
