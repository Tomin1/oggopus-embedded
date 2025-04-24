#!/usr/bin/env python3
# Copyright (c) 2025 Tomi Leppänen
# SPDX-License-Identifier: MIT
#
# Run benchmark and calculate whether playback is real time.

import argparse
import pandas as pd
import io
import serial


def read_table(sio):
    data = io.StringIO()
    while True:
        line = sio.readline()
        if line.startswith("frequency"):
            data.write(line)
            break
    while True:
        line = sio.readline()
        if line.startswith("frequency"):
            break
        data.write(line)
    data.seek(0)
    return pd.read_csv(data, skipinitialspace=True, lineterminator="\n")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "serial_port",
        metavar="SERIAL_PORT",
        help="Serial port device (like /dev/ttyACMx)",
    )
    parser.add_argument(
        "-b",
        "--baudrate",
        type=int,
        default=115200,
        help="Baudrate for the serial port",
    )
    parser.add_argument(
        "-s", "--skip", action="store_true", default=False, help="Skip initial data"
    )
    args = parser.parse_args()

    with serial.Serial(args.serial_port, baudrate=args.baudrate) as port:
        sio = io.TextIOWrapper(io.BufferedReader(port))
        if args.skip:
            read_table(sio)
        df = read_table(sio)

    decode_speed = df["sample time"] / df["decode time"]
    if (decode_speed < 1).any():
        print("Too slow to decode some frames")
    else:
        print("Fast enough to decode all frames")

    print(f"Mean decode speed: {decode_speed.mean() * 100 :.1f} %")
    print(f"Variance of decode speed: {decode_speed.var() * 100:.3f} %")
    print(f"Minimum decode speed: {decode_speed.min() * 100 :.1f} %")
    print(f"Maximum decode speed: {decode_speed.max() * 100 :.1f} %")

    # Skip first few packets as they are not representative
    playback_speed = df["sample time"][3:] / df["playback time"][3:]
    print(f"Mean playback speed: {playback_speed.mean() * 100 :.1f} %")
    print(f"Variance of playback speed: {playback_speed.var() * 100:.3f} %")
    print(f"Minimum playback speed: {playback_speed.min() * 100 :.1f} %")
    print(f"Maximum playback speed: {playback_speed.max() * 100 :.1f} %")


if __name__ == "__main__":
    main()
