#!/usr/bin/env python3
# Copyright (c) 2025 Tomi Leppänen
# SPDX-License-Identifier: MIT
#
# Run benchmark and calculate whether playback is real time.

import argparse
import pandas as pd
import io
import serial
from pathlib import Path


def Bitrate(s):
    s = s.lower()
    if s in ["8k", "12k", "16k", "24k", "32k", "48k", "64k", "custom"]:
        return s
    raise ValueError


def Frequency(s):
    s = s.lower()
    if s in ["8khz", "12khz", "16khz", "24khz", "48khz"]:
        return s
    raise ValueError


def build_command(args):
    if args.play_only:
        command = b"play"
    else:
        command = b"benchmark"

    if args.silent:
        command += b" -s"

    if args.freq:
        command += f" {args.freq}".encode()

    if args.bitrate:
        command += f" {args.bitrate}".encode()

    return command + b"\r\n"


def get_output(command, serial_port, baudrate):
    with serial.Serial(serial_port, baudrate=baudrate, timeout=10) as s:
        s.reset_input_buffer()
        s.write(b"\r\n")
        s.flush()
        content = s.read_until(b"> ")
        assert content.endswith(b"> ")
        s.write(command)
        s.flush()
        content = s.read_until(b"\r\n")
        assert content == command
        content = bytes()
        while not content.endswith(b"> "):
            content += s.read_until(b"> ")
        return content


def read_table(content):
    data = io.StringIO(content)
    return pd.read_csv(data, skipinitialspace=True, lineterminator="\n")


def print_report(df):
    decode_speed = df["sample time"] / df["decode time"]
    if (decode_speed < 1).any():
        print("Too slow to decode some frames")
    else:
        print("Fast enough to decode all frames")

    print(f"Mean decode speed: {decode_speed.mean() * 100 :.1f} %")
    print(f"Variance of decode speed: {decode_speed.var() * 100:.3f} %pt.")
    print(f"Minimum decode speed: {decode_speed.min() * 100 :.1f} %")
    print(f"Maximum decode speed: {decode_speed.max() * 100 :.1f} %")

    # Skip first few packets as they are not representative
    playback_speed = df["sample time"][3:] / df["playback time"][3:]
    print(f"Mean playback speed: {playback_speed.mean() * 100 :.1f} %")
    print(f"Variance of playback speed: {playback_speed.var() * 100:.3f} %pt.")
    print(f"Minimum playback speed: {playback_speed.min() * 100 :.1f} %")
    print(f"Maximum playback speed: {playback_speed.max() * 100 :.1f} %")


def main():
    parser = argparse.ArgumentParser(
        epilog="*) unsupported sampling rate for MAX38357A"
    )
    parser.add_argument(
        "serial_port",
        metavar="SERIAL_PORT",
        help="Serial port device (like /dev/ttyACMx)",
    )
    play_group = parser.add_mutually_exclusive_group()
    play_group.add_argument(
        "-s",
        "--silent",
        action="store_true",
        default=False,
        help="Don't play audio while benchmarking",
    )
    play_group.add_argument(
        "-p",
        "--play-only",
        action="store_true",
        default=False,
        help="Don't measure, only play audio",
    )
    bitrate_group = parser.add_mutually_exclusive_group()
    bitrate_group.add_argument(
        "-b",
        "--bitrate",
        type=Bitrate,
        default=None,
        help="Select bitrate to decode, one of 8k, 12k, 16k, 24k, 32k, 48k and 64k, or 'custom'",
    )
    bitrate_group.add_argument(
        "-m",
        "--measure",
        action="store_true",
        default=False,
        help="Test different bitrates to find the best that decodes real-time",
    )
    parser.add_argument(
        "-f",
        "--freq",
        type=Frequency,
        default=None,
        help="Sampling rate for playback, one of 8khz, 12khz*, 16khz, 24khz* and 48khz",
    )
    parser.add_argument(
        "--print-table",
        action="store_true",
        default=False,
        help="Print data frames collected from the device",
    )
    parser.add_argument(
        "--save-table",
        type=Path,
        default=None,
        help="Save data frames to file(s)",
    )
    parser.add_argument(
        "--baudrate",
        type=int,
        default=115200,
        help="Baudrate for the serial port",
    )
    args = parser.parse_args()

    if args.measure:
        if args.save_table is not None:
            args.save_table = open(args.save_table, "w")
        data = []
        for bitrate in ["8k", "12k", "16k", "24k", "32k", "48k", "64k"]:
            args.bitrate = bitrate
            command = build_command(args)
            content = get_output(command, args.serial_port, args.baudrate)
            df = read_table(content[:-2].decode().replace("\r\n", "\n"))
            if args.print_table:
                print(f"With {bitrate[:-1]} kb/s")
                print(df.to_string())
            if args.save_table is not None:
                args.save_table.write(f"With {bitrate[:-1]} kb/s\n")
                df.to_csv(args.save_table)
                args.save_table.write("\n\n")
            decode_speed = df["sample time"] / df["decode time"]
            data.append(
                [
                    bitrate,
                    (decode_speed >= 1).all(),
                    decode_speed.mean(),
                    decode_speed.var(),
                    decode_speed.min(),
                    decode_speed.max(),
                ]
            )
        if args.save_table:
            args.save_table.close()
        df = pd.DataFrame(
            data,
            columns=["bitrate", "real time", "mean", "variance", "minimum", "maximum"],
        )
        df["real time"] = df["real time"].map(lambda x: "yes" if x else "no")
        df["mean"] = df["mean"].map(lambda x: f"{x * 100:.1f} %")
        df["variance"] = df["variance"].map(lambda x: f"{x * 100:.3f} %pt.")
        df["minimum"] = df["minimum"].map(lambda x: f"{x * 100:.1f} %")
        df["maximum"] = df["maximum"].map(lambda x: f"{x * 100:.1f} %")
        print(df)
    else:
        command = build_command(args)
        content = get_output(command, args.serial_port, args.baudrate)
        if content.startswith(b"Invalid command"):
            print(f"Cannot play '{args.bitrate}'")
            return
        if not args.play_only:
            df = read_table(content[:-2].decode().replace("\r\n", "\n"))
            if args.print_table:
                print(df.to_string())
            if args.save_table is not None:
                df.to_csv(args.save_table)
            print_report(df)


if __name__ == "__main__":
    main()
