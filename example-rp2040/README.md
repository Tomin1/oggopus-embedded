rp2040 example
==============
This is a small example for rp2040 microcontroller found in Raspberry Pico. It
uses I2S to play (mono) audio samples and also serves as a benchmark for Opus
decoding.

Building
--------
You must build this inside the directory and not in the workspace directory,
otherwise cargo will not see the required configuration. Defaults to
thumbv6m-none-eabi target for Cortex-M0+ found in rp2040.

Hardware setup
--------------
Benchmarking doesn't require any hardware setup other than connecting Raspberry
Pico to the computer with Pico Debug Probe and even the probe is optional. If
you want to also play music, you'll need a DAC and a speaker.

Other boards than the official Raspberry Pico can work but you need to figure
out the differences yourself. The benefits of other boards include bigger flash
size which allows to fit bigger samples.

MAX38357A is the only supported DAC. [Adafruit MAX38357A breakout
board](https://www.adafruit.com/product/3006) is recommended. MAX98360A may
also work but you need to figure out the pinout yourself and playback cannot be
skipped while benchmarking due to lack of SD pin. You may skip the DAC if you
don't intend to play any audio.

Connect the pins of MAX38357A in the following way to Raspberry Pico:

| DAC pin | Raspberry Pico pin |
| ------- | ------------------ |
| `LRC`   | GPIO 19            |
| `BCLK`  | GPIO 18            |
| `DIN`   | GPIO 17            |
| `GAIN`  | 3V3 (optionally)   |
| `SD`    | GPIO 16            |
| `GND`   | GND                |
| `VIN`   | 3V3                |

Also connect a speaker to the output.

Custom sample
-------------
Enable `custom` feature to include a custom sample in the binary. Place the
file to `src/custom.opus`. It may be mono or stereo sample with channel mapping
family 0 (usually the default when creating mono or stereo audio file) and it
must fit on the flash (2 MB on Raspberry Pico) together with the program so it
can be at most about 1.5 MB. That is enough to fit a short song with very good
quality or a longer one with suitable quality for the purpose.

Usage
-----
Build and flash this to device with Pico Debug Probe with `cargo run --release`
in this directory. After that use the provided python script to run benchmarks
and play audio, or use serial terminal on the USB serial device directly.

### Via python script
This is the recommended way to test the device.

Run `python3 benchmark.py --help` to get all of the usage information.

To measure measuring different bitrates, run the following command:
```sh
python3 benchmark.py -s -m
```

This runs a test for all included bitrates (sans custom) and measures how
quickly relative to playback speed they could be decoded without playing
through a speaker.

In the output 100 % speed means real-time decoding. If any frame is decoded
slower than that, the playback is not considered real-time. Variance is
reported as percentage points. rp2040 should be capable of decoding all the
provided bitrates.

To play your own custom sample and print and save the resulting table of
decoding and playback time use the following command:
```sh
python3 benchmark.py -b custom --print-table --save-table custom.csv
```

### Via serial device
You can use for example minicom on Linux:
```sh
minicom -D /dev/ttyACM0
```

Press enter to get a new command prompt. Type `help` enter to get usage
information. Backspace is supported even if it doesn't erase any text. Arrow
keys are not supported.

Note that unlike the python script, this is not capable of calculating
statistics. It only prints tables of decoding and playback time.

### Usage without Debug Probe
If you don't have a debug probe or another Pico with debug probe firmware, you
may use `elf2uf2-rs` to convert the binary built with `cargo build --release`
and write the file to the board via USB. The debug probe is only required if
you intend to develop.

License
-------
This example is MIT licensed. The license is inherited from the embassy
examples that this builds on. See COPYING for more infomation. Dependency
crates have their own licenses.
