/*
 * Copyright (c) 2025 Tomi Leppänen
 * SPDX-License-Identifier: BSD-3-Clause
 *
 * A simple example player that uses oggopus-embedded and opus-embedded.
 */

use alsa::{
    pcm::{Access, Format, HwParams},
    Direction, ValueOr, PCM,
};
use oggopus_embedded::prelude::*;
use opus_embedded::prelude::*;

fn main() -> Result<(), Box<dyn core::error::Error>> {
    let pcm = PCM::new("default", Direction::Playback, false)?;
    let hwp = HwParams::any(&pcm)?;

    let stream_content = std::fs::read(std::env::args().nth(1).ok_or("Argument missing")?)?;

    let stream: Bitstream = Bitstream::new(stream_content.as_slice());
    let (reader, header) = stream
        .reader()
        .read_header()
        .inspect_err(|err| println!("Failed to read header or comments packet: {err:?}"))?;
    let Either::Continued(mut reader) = reader else {
        return Err("Stream ended after header or comments packet".into());
    };
    let channels = match header.channels {
        ChannelMapping::Family0 { channels } => channels,
        _ => return Err("Unsupported channel mapping family".into()),
    };
    let sample_rate = SamplingRate::closest(header.sample_rate.try_into().unwrap());

    hwp.set_channels(channels.into())?;
    hwp.set_rate(sample_rate as u32, ValueOr::Nearest)?;
    hwp.set_format(Format::s16())?;
    hwp.set_access(Access::RWInterleaved)?;
    pcm.hw_params(&hwp)?;
    let io = pcm.io_i16()?;

    let hwp = pcm.hw_params_current()?;
    let swp = pcm.sw_params_current()?;
    swp.set_start_threshold(hwp.get_buffer_size()?)?;
    pcm.sw_params(&swp)?;
    let channels = Channels::try_from(channels).unwrap();

    println!(
        "Playing in {:?} at rate of {}",
        channels, sample_rate as i32
    );

    let mut decoder = Decoder::new(sample_rate, channels)?;
    let mut output = Vec::default();
    let mut total = 0;

    if sample_rate as u32 != hwp.get_rate()? {
        return Err("Could not set matching sampling rate".into());
    }

    loop {
        let mut sum = 0;
        let (new_reader, mut packets) = reader
            .next_packets::<1024>()
            .inspect_err(|err| println!("Failed to read packets: {err:?}"))?;

        while let Some(packet) = packets.next() {
            output.resize(decoder.get_nb_samples_total(packet.data)?, 0i16);
            let output = decoder.decode(packet.data, output.as_mut_slice())?;
            io.writei(output)?;
            sum += output.len() / channels as usize;
        }
        println!("Decoded {sum} samples");
        total += sum;
        match new_reader {
            Either::Ended(next_reader) => {
                if let Some(next_reader) = next_reader.next_reader() {
                    let (Either::Continued(next_reader), header) =
                        next_reader.read_header().inspect_err(|err| {
                            println!("Failed to read header or comments packet: {err:?}")
                        })?
                    else {
                        return Err("New stream ended after header or comments packet".into());
                    };
                    println!("New stream started");
                    match header.channels {
                        ChannelMapping::Family0 { channels } => {
                            let channels = Channels::try_from(channels).unwrap();
                            decoder = Decoder::new(sample_rate, channels)?;
                        }
                        _ => return Err("Unsupported channel mapping family".into()),
                    };
                    reader = next_reader;
                } else {
                    break;
                }
            }
            Either::Continued(new_reader) => {
                reader = new_reader;
            }
        }
    }

    println!("Total decoded samples: {total}");

    pcm.wait(None)?;
    pcm.drain()?;
    println!("Playback finished");

    Ok(())
}
