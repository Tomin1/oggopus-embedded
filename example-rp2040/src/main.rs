/*
 * Copyright (c) Embassy project contributors
 * Copyright (c) 2025 Tomi Leppänen
 *
 * SPDX-License-Identifier: MIT
 *
 * Adapted from embassy examples:
 * https://github.com/embassy-rs/embassy/blob/9d62fba7d2e6b5d3bcb54770ffd031c1f3dafc84/examples/rp/src/bin/pio_i2s.rs
 * and
 * https://github.com/embassy-rs/embassy/blob/9d62fba7d2e6b5d3bcb54770ffd031c1f3dafc84/examples/rp/src/bin/usb_serial.rs
 */

//! Example for I2S playback with oggopus-embedded and opus-embedded crates on RP2040.
//! Outputs statistics over USB serial.
//!
//! Connect the i2s DAC (MAX38357A) as follows:
//!   bclk : GPIO 18
//!   lrc  : GPIO 19
//!   din  : GPIO 17
//!   sd   : GPIO 16

#![no_std]
#![no_main]

use core::mem;
use core::time::Duration;
use defmt::{info, panic, unwrap};
use embassy_executor::Spawner;
use embassy_rp::bind_interrupts;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::peripherals::{DMA_CH0, PIN_17, PIN_18, PIN_19, PIO0, USB};
use embassy_rp::pio::{InterruptHandler as PioInterruptHandler, Pio};
use embassy_rp::pio_programs::i2s::{PioI2sOut, PioI2sOutProgram};
use embassy_rp::usb::{Driver, InterruptHandler as UsbInterruptHandler};
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use embassy_usb::UsbDevice;
use embassy_usb::class::cdc_acm::{CdcAcmClass, State};
use embassy_usb::driver::EndpointError;
use numtoa::NumToA;
use oggopus_embedded::{Bitstream, opus::ChannelMapping, states::Either};
use opus_embedded::{Channels, Decoder, SamplingRate};
use static_cell::StaticCell;
use tinyvec::ArrayVec;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct PioIrqs {
    PIO0_IRQ_0 => PioInterruptHandler<PIO0>;
});

bind_interrupts!(struct UsbIrqs {
    USBCTRL_IRQ => UsbInterruptHandler<USB>;
});

const BIT_DEPTH: u32 = 16;
const OUTPUT_CHANNELS: u32 = 2;

async fn print_header<'a>(
    class: &mut CdcAcmClass<'a, embassy_rp::usb::Driver<'a, USB>>,
) -> Result<(), Disconnected> {
    class
        .write_packet(b"frequency, channels, samples, ")
        .await?;
    class
        .write_packet(b"\"sample time\", \"decode time\", \"playback time\"\r\n")
        .await?;
    Ok(())
}

fn create_row_header(frequency: SamplingRate, channels: u8) -> ArrayVec<[u8; 64]> {
    let mut vec = ArrayVec::new();
    let mut buffer = [0u8; 20];
    vec.extend_from_slice((frequency as u32).numtoa(10, &mut buffer));
    vec.extend_from_slice(b", ");
    vec.extend_from_slice(channels.numtoa(10, &mut buffer));
    vec.extend_from_slice(b", ");
    vec
}

const fn get_sample_time(frequency: SamplingRate) -> Duration {
    Duration::from_nanos(1_000_000_000 / frequency as u64)
}

fn create_samples_row_header(samples: usize, frequency: SamplingRate) -> ArrayVec<[u8; 64]> {
    let mut vec = ArrayVec::new();
    let mut buffer = [0u8; 20];
    vec.extend_from_slice(samples.numtoa(10, &mut buffer));
    vec.extend_from_slice(b", ");
    let sample_time = get_sample_time(frequency) * samples as u32;
    assert_eq!(sample_time.as_secs(), 0);
    vec.extend_from_slice((sample_time.subsec_micros()).numtoa(10, &mut buffer));
    vec.extend_from_slice(b", ");
    vec
}

async fn print_time<'a>(
    class: &mut CdcAcmClass<'a, embassy_rp::usb::Driver<'a, USB>>,
    row_header: &[u8],
    samples_header: &[u8],
    decode_time: EmbassyDuration,
    playback_time: EmbassyDuration,
) -> Result<(), Disconnected> {
    let mut buffer = [0u8; 20];
    class.write_packet(row_header).await?;
    class.write_packet(samples_header).await?;
    class
        .write_packet(decode_time.as_micros().numtoa(10, &mut buffer))
        .await?;
    class.write_packet(b", ").await?;
    class
        .write_packet(playback_time.as_micros().numtoa(10, &mut buffer))
        .await?;
    class.write_packet(b"\r\n").await?;
    Ok(())
}

#[derive(Debug)]
struct Disconnected {}

impl From<EndpointError> for Disconnected {
    fn from(val: EndpointError) -> Self {
        match val {
            EndpointError::BufferOverflow => panic!("Buffer overflow"),
            EndpointError::Disabled => Disconnected {},
        }
    }
}

#[derive(Debug)]
enum BitstreamType {
    Bs8k,
    Bs12k,
    Bs16k,
    Bs24k,
    Bs32k,
    Bs48k,
    Bs64k,
    #[cfg(feature = "custom")]
    Custom,
}

#[derive(Debug, PartialEq)]
enum Task {
    BenchmarkAndPlay,
    Benchmark,
    Play,
}

#[derive(Debug)]
struct Selections {
    task: Task,
    sampling_rate: SamplingRate,
    bitstream_type: BitstreamType,
}

impl Selections {
    async fn get<'a>(
        class: &mut CdcAcmClass<'a, embassy_rp::usb::Driver<'a, USB>>,
    ) -> Result<Self, Disconnected> {
        let mut buffer = ArrayVec::<[u8; 128]>::default();
        loop {
            info!("Prompting");
            class.write_packet(b"> ").await?;
            let mut buf = [0; 64];
            'buffering: loop {
                info!("Waiting data");
                let n = class.read_packet(&mut buf).await?;
                if n > 0 {
                    class.write_packet(&buf[..n]).await?;
                    if buffer.len() + n > buffer.capacity() {
                        // Cannot fit to command buffer => invalid command
                        info!("Buffer would overflow {}", buffer.len() + n);
                        buffer.clear();
                        break;
                    }
                    for c in buf[..n].iter() {
                        if *c == b'\n' || *c == b'\r' {
                            // UTF-8 test
                            if let Ok(command) = core::str::from_utf8(buffer.as_slice()) {
                                info!("Got command '{}'", command);
                            } else {
                                // Weird characters => invalid command
                                info!("Got weird characters: {}", buffer.as_slice());
                                buffer.clear();
                            }
                            break 'buffering;
                        } else if *c == 0x08 {
                            // Backspace
                            buffer.pop();
                        } else {
                            buffer.push(*c);
                        }
                    }
                }
            }
            class.write_packet(b"\r\n").await?;
            let mut command = buffer.as_ref().split(|c| *c == b' ');
            let mut task = match command.next() {
                Some(b"benchmark") => Task::BenchmarkAndPlay,
                Some(b"play") => Task::Play,
                Some(b"help") => {
                    class.write_packet(b"Commands:\r\n").await?;
                    class
                        .write_packet(b"benchmark [-s] [8khz|16khz|24khz|48khz]")
                        .await?;
                    #[cfg(feature = "custom")]
                    class
                        .write_packet(b" [8k|12k|16k|24k|32k|64k|custom]\r\n")
                        .await?;
                    #[cfg(not(feature = "custom"))]
                    class.write_packet(b" [8k|12k|16k|24k|32k|64k]\r\n").await?;
                    class.write_packet(b"play [8khz|16khz|24khz|48khz]").await?;
                    #[cfg(feature = "custom")]
                    class
                        .write_packet(b" [8k|12k|16k|24k|32k|64k|custom]\r\n")
                        .await?;
                    #[cfg(not(feature = "custom"))]
                    class.write_packet(b" [8k|12k|16k|24k|32k|64k]\r\n").await?;
                    buffer.clear();
                    continue;
                }
                _ => {
                    class.write_packet(b"Invalid command\r\n").await?;
                    buffer.clear();
                    continue;
                }
            };
            let mut arg = command.next();
            if task == Task::BenchmarkAndPlay && arg == Some(b"-s") {
                // Silent benchmark
                task = Task::Benchmark;
                arg = command.next();
            }
            let sampling_rate = match arg {
                Some(freq) if freq.ends_with(b"khz") => {
                    let sampling_rate = match freq {
                        b"8khz" => SamplingRate::F8k,
                        b"16khz" => SamplingRate::F16k,
                        b"24khz" => SamplingRate::F24k,
                        b"48khz" => SamplingRate::F48k,
                        _ => {
                            class.write_packet(b"Invalid command\r\n").await?;
                            buffer.clear();
                            continue;
                        }
                    };
                    arg = command.next();
                    sampling_rate
                }
                _ => {
                    if cfg!(feature = "default-to-48khz") {
                        SamplingRate::F48k
                    } else {
                        SamplingRate::F16k
                    }
                }
            };
            let bitstream_type = match arg {
                Some(b"8k") | None => BitstreamType::Bs8k,
                Some(b"12k") => BitstreamType::Bs12k,
                Some(b"16k") => BitstreamType::Bs16k,
                Some(b"24k") => BitstreamType::Bs24k,
                Some(b"32k") => BitstreamType::Bs32k,
                Some(b"48k") => BitstreamType::Bs48k,
                Some(b"64k") => BitstreamType::Bs64k,
                #[cfg(feature = "custom")]
                Some(b"custom") => BitstreamType::Custom,
                _ => {
                    class.write_packet(b"Invalid command\r\n").await?;
                    buffer.clear();
                    continue;
                }
            };
            buffer.clear();
            return Ok(Self {
                task,
                sampling_rate,
                bitstream_type,
            });
        }
    }

    fn get_bitstream(&self) -> Bitstream<'static> {
        use BitstreamType::*;
        match self.bitstream_type {
            Bs8k => Bitstream::new(include_bytes!("tone_440_8k.opus")),
            Bs12k => Bitstream::new(include_bytes!("tone_440_12k.opus")),
            Bs16k => Bitstream::new(include_bytes!("tone_440_16k.opus")),
            Bs24k => Bitstream::new(include_bytes!("tone_440_24k.opus")),
            Bs32k => Bitstream::new(include_bytes!("tone_440_32k.opus")),
            Bs48k => Bitstream::new(include_bytes!("tone_440_48k.opus")),
            Bs64k => Bitstream::new(include_bytes!("tone_440_64k.opus")),
            #[cfg(feature = "custom")]
            Custom => Bitstream::new(include_bytes!("custom.opus")),
        }
    }

    fn benchmark(&self) -> bool {
        match &self.task {
            Task::BenchmarkAndPlay | Task::Benchmark => true,
            Task::Play => false,
        }
    }

    fn play(&self) -> bool {
        match &self.task {
            Task::BenchmarkAndPlay | Task::Play => true,
            Task::Benchmark => false,
        }
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    // Led to indicate progress
    let mut led = Output::new(p.PIN_25, Level::Low);

    // Usb serial setup for benchmarking
    info!("Initializing usb");
    let driver = Driver::new(p.USB, UsbIrqs);

    // Create embassy-usb Config
    let config = {
        let mut config = embassy_usb::Config::new(0xc0de, 0xcafe);
        config.manufacturer = Some("None");
        config.product = Some("oggopus-embedded example");
        config.serial_number = Some("12345678");
        config.max_power = 100;
        config.max_packet_size_0 = 64;
        config
    };

    // Create embassy-usb DeviceBuilder using the driver and config.
    // It needs some buffers for building the descriptors.
    let mut builder = {
        static CONFIG_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
        static BOS_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
        static CONTROL_BUF: StaticCell<[u8; 64]> = StaticCell::new();

        let builder = embassy_usb::Builder::new(
            driver,
            config,
            CONFIG_DESCRIPTOR.init([0; 256]),
            BOS_DESCRIPTOR.init([0; 256]),
            &mut [], // no msos descriptors
            CONTROL_BUF.init([0; 64]),
        );
        builder
    };

    // Create classes on the builder.
    let mut class = {
        static STATE: StaticCell<State> = StaticCell::new();
        let state = STATE.init(State::new());
        CdcAcmClass::new(&mut builder, state, 64)
    };

    // Build the builder.
    let usb = builder.build();

    // Run the USB device.
    unwrap!(spawner.spawn(usb_task(usb)));

    // Setup SD pin
    let mut sd = Output::new(p.PIN_16, Level::Low);

    // Create two audio buffers (back and front) which will take turns being
    // filled with new audio data and being sent to the PIO FIFO using dma.
    // This can fit one 20 ms sample at 48 000 Hz plus quite a bit of padding.
    const BUFFER_SIZE: usize = 1_536;
    static DMA_BUFFER: StaticCell<[u32; BUFFER_SIZE * 2]> = StaticCell::new();
    let dma_buffer = DMA_BUFFER.init_with(|| [0u32; BUFFER_SIZE * 2]);

    loop {
        // Wait for USB in case it had been disconnected
        info!("Waiting for usb connection");
        class.wait_connection().await;

        let selections = match Selections::get(&mut class).await {
            Ok(selections) => selections,
            Err(_) => {
                // Wrap back to waiting for connections on USB disconnects
                continue;
            }
        };

        // Setup pio state machine for i2s output
        info!("Initializing pio");

        // Set SD pin to high for playback
        if selections.play() {
            sd.set_high();
        } else {
            sd.set_low();
        }

        let Pio {
            mut common, sm0, ..
        } = Pio::new(unsafe { PIO0::steal() }, PioIrqs);
        let program = PioI2sOutProgram::new(&mut common);

        let bit_clock_pin = unsafe { PIN_18::steal() };
        let left_right_clock_pin = unsafe { PIN_19::steal() };
        let data_pin = unsafe { PIN_17::steal() };
        let mut i2s = PioI2sOut::new(
            &mut common,
            sm0,
            unsafe { DMA_CH0::steal() },
            data_pin,
            bit_clock_pin,
            left_right_clock_pin,
            selections.sampling_rate as u32,
            BIT_DEPTH,
            OUTPUT_CHANNELS,
            &program,
        );

        // Get opus stream to decode
        info!("Initializing stream");
        let stream = selections.get_bitstream();

        // Check headers that they are as expected
        let (reader, header) = stream.reader().read_header().unwrap();
        let Either::Continued(mut reader) = reader else {
            panic!("Stream ended after header or comments packet");
        };
        let ChannelMapping::Family0 { channels } = header.channels else {
            panic!("Unsupported channel mapping family");
        };
        let mut pre_skip = header.pre_skip as usize;

        let mut row_header = ArrayVec::default();
        if selections.benchmark() {
            row_header = create_row_header(selections.sampling_rate, channels);
        }

        // Create decoder, use stereo so we don't need to copy to get u32.
        // It could be useful in some cases, such as if we wanted to adjust volume.
        info!("Initializing decoding");
        let mut decoder = Decoder::new(selections.sampling_rate, Channels::Stereo).unwrap();
        let (mut back_buffer, mut front_buffer) = dma_buffer.split_at_mut(BUFFER_SIZE);
        let mut old_samples = 0;
        let mut samples_header = ArrayVec::default();

        // Setup front buffer with silence
        front_buffer.fill(0);
        let mut samples = front_buffer.len();

        // Trigger transfer of empty front buffer data to the PIO FIFO
        // but don't await the returned future, yet
        let mut dma_future = i2s.write(&front_buffer[0..samples]);

        info!("Decoding the sample");
        if selections.benchmark() && print_header(&mut class).await.is_err() {
            continue;
        }
        let mut prev_instant = Instant::now();
        'inner: loop {
            // Toggle led just to indicate that progress is being made
            led.toggle();

            // Get ogg packets to decode
            let (new_reader, mut packets) = reader.next_packets::<1024>().unwrap();
            while let Some(packet) = packets.next() {
                // Fill back buffer with fresh audio samples before awaiting the dma future
                info!("Decode new packet");
                let start_of_decode = Instant::now();
                assert!(decoder.get_nb_samples(packet.data).unwrap() <= back_buffer.len());
                let decode_buffer: &mut [i16] = unsafe {
                    let length = back_buffer.len();
                    let buffer_ptr = back_buffer as *mut [u32] as *mut u32;
                    // SAFETY: i16 is half the size of u32
                    core::slice::from_raw_parts_mut(
                        // SAFETY: Raw data, alignment for i16 is less strict than u32
                        core::mem::transmute::<*mut u32, *mut i16>(buffer_ptr),
                        length * 2,
                    )
                };
                // Actual decoding happens here, this is the slow part
                samples = match decoder.decode(packet.data, decode_buffer) {
                    Err(_) => {
                        let _ = class.write_packet(b"Decoding failed\r\n").await;
                        break 'inner;
                    }
                    Ok(output) => output.len() / 2, // Two channels => samples per frame
                };
                let end_of_decode = Instant::now();
                if selections.benchmark() {
                    if samples != old_samples {
                        samples_header =
                            create_samples_row_header(samples, selections.sampling_rate);
                        old_samples = samples;
                    }
                    if print_time(
                        &mut class,
                        &row_header,
                        &samples_header,
                        end_of_decode - start_of_decode,
                        end_of_decode - prev_instant,
                    )
                    .await
                    .is_err()
                    {
                        break 'inner;
                    };
                } else {
                    // TBH I'm not quite sure why this is needed
                    let _ = class.write_packet(&[]).await;
                }
                prev_instant = end_of_decode;

                // Now await the dma future. This ensures that the previous buffer has been
                // consumed and there is now DMA QUEUE DEPTH / SAMPLING RATE, e.g. 8 / 48,000 Hz =
                // 166 µs of time to send the next so it's queued immediately and get 20 ms time to
                // process the next packet. One could also decode multiple packets at once and use
                // some other multiple of 2.5 ms instead (this is determined by the encoder).
                dma_future.await;
                mem::swap(&mut back_buffer, &mut front_buffer);
                dma_future = i2s.write(&front_buffer[pre_skip..pre_skip + samples]);
                pre_skip = 0;
            }

            // Prepare reader for the next round if there is any
            match new_reader {
                Either::Ended(new_reader) => {
                    // Stream ended, start over
                    if !new_reader.has_more() {
                        info!("Finished playback");
                        break 'inner;
                    } else {
                        // We don't expect this with our samples. In principle there could be
                        // another opus stream following in the same Ogg stream.
                        panic!("Unexpected next stream!");
                    }
                }
                Either::Continued(new_reader) => {
                    info!("Continuing to next packets");
                    reader = new_reader;
                }
            }
        }

        // Prepare buffer of silence
        back_buffer.fill(0);
        // Schedule and play it
        dma_future.await;
        i2s.write(back_buffer).await;
        // Wait until all has been played
        Timer::after_millis(1).await;
        // TODO: Check that this drives the other pins low too
        // Turn off the amplifier
        sd.set_low();
    }
}

type MyUsbDriver = Driver<'static, USB>;
type MyUsbDevice = UsbDevice<'static, MyUsbDriver>;

#[embassy_executor::task]
async fn usb_task(mut usb: MyUsbDevice) -> ! {
    usb.run().await
}
