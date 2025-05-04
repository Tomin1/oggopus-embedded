rp2040 example
==============
This is a small example for rp2040 microcontroller found in Raspberry Pico. It
uses I2S to play (mono) audio samples.

Building
--------
You must build this inside the directory and not in the workspace directory,
otherwise cargo will not see the required configuration. Defaults to
thumbv6m-none-eabi target for Cortex-M0+ found in rp2040.
