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
use embassy_rp::peripherals::{PIO0, USB};
use embassy_rp::pio::{InterruptHandler as PioInterruptHandler, Pio};
use embassy_rp::pio_programs::i2s::{PioI2sOut, PioI2sOutProgram};
use embassy_rp::usb::{Driver, InterruptHandler as UsbInterruptHandler};
use embassy_time::{Duration as EmbassyDuration, Instant};
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

const SAMPLING_RATE: SamplingRate = SamplingRate::F16k;
const BIT_DEPTH: u32 = 16;
const OUTPUT_CHANNELS: u32 = 2;

async fn print_header<'a>(
    class: &mut CdcAcmClass<'a, embassy_rp::usb::Driver<'a, USB>>,
) -> Result<(), Disconnected> {
    class
        .write_packet(b"frequency, channels, samples, ")
        .await?;
    class
        .write_packet(b"\"sample time\", \"decode time\", \"playback time\"\n")
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
    class.write_packet(b"\n").await?;
    Ok(())
}

struct Disconnected {}

impl From<EndpointError> for Disconnected {
    fn from(val: EndpointError) -> Self {
        match val {
            EndpointError::BufferOverflow => panic!("Buffer overflow"),
            EndpointError::Disabled => Disconnected {},
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

    // Setup pio state machine for i2s output
    info!("Initializing pio");
    let Pio {
        mut common, sm0, ..
    } = Pio::new(p.PIO0, PioIrqs);

    let bit_clock_pin = p.PIN_18;
    let left_right_clock_pin = p.PIN_19;
    let data_pin = p.PIN_17;

    let program = PioI2sOutProgram::new(&mut common);
    let mut i2s = PioI2sOut::new(
        &mut common,
        sm0,
        p.DMA_CH0,
        data_pin,
        bit_clock_pin,
        left_right_clock_pin,
        SAMPLING_RATE as u32,
        BIT_DEPTH,
        OUTPUT_CHANNELS,
        &program,
    );

    info!("Initializing decoding");

    // Set SD pin up
    let _sd = Output::new(p.PIN_16, Level::High);

    // Include opus stream to decode
    const STREAM: Bitstream = Bitstream::new(include_bytes!("tone_440_8k.opus"));

    // Create two audio buffers (back and front) which will take turns being
    // filled with new audio data and being sent to the PIO FIFO using dma.
    // This can fit one 20 ms sample at 48 000 Hz.
    const BUFFER_SIZE: usize = 960;
    static DMA_BUFFER: StaticCell<[u32; BUFFER_SIZE * 2]> = StaticCell::new();
    let dma_buffer = DMA_BUFFER.init_with(|| [0u32; BUFFER_SIZE * 2]);

    loop {
        // Check headers that they are as expected
        let (reader, header) = STREAM.reader().read_header().unwrap();
        let Either::Continued(mut reader) = reader else {
            panic!("Stream ended after header or comments packet");
        };
        let ChannelMapping::Family0 { channels } = header.channels else {
            panic!("Unsupported channel mapping family");
        };
        assert_eq!(channels, 1);
        let mut pre_skip_done = false;
        let mut pre_skip = 0usize;

        let row_header = create_row_header(SAMPLING_RATE, channels);

        // Create decoder, use stereo so we don't need to copy to get u32.
        // It could be useful in some cases, such as if we wanted to adjust volume.
        let mut decoder = Decoder::new(SAMPLING_RATE, Channels::Stereo).unwrap();
        let (mut back_buffer, mut front_buffer) = dma_buffer.split_at_mut(BUFFER_SIZE);
        let mut old_samples = 0;
        let mut samples_header = ArrayVec::default();

        info!("Waiting for usb connection");
        class.wait_connection().await;

        // Setup front buffer with silence
        front_buffer.fill(0);
        let mut samples = front_buffer.len();

        info!("Decoding the sample");
        let _ = print_header(&mut class).await;
        let mut prev_instant = Instant::now();
        'inner: loop {
            // Toggle led just to indicate that progress is being made
            led.toggle();

            // Get ogg packets to decode
            let (new_reader, mut packets) = reader.next_packets::<1024>().unwrap();
            while let Some(packet) = packets.next() {
                // Trigger transfer of front buffer data to the PIO FIFO
                // but don't await the returned future, yet
                let dma_future = i2s.write(&front_buffer[pre_skip..samples]);
                if !pre_skip_done {
                    pre_skip = header.pre_skip as usize;
                    pre_skip_done = true;
                } else {
                    pre_skip = 0;
                }

                // Fill back buffer with fresh audio samples before awaiting the dma future
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
                samples = decoder.decode(packet.data, decode_buffer).unwrap();
                let end_of_decode = Instant::now();
                if samples != old_samples {
                    samples_header = create_samples_row_header(samples, SAMPLING_RATE);
                    old_samples = samples;
                }
                let _ = print_time(
                    &mut class,
                    &row_header,
                    &samples_header,
                    end_of_decode - start_of_decode,
                    end_of_decode - prev_instant,
                )
                .await;

                // This needs to schedule next transfer in DMA QUEUE DEPTH / SAMPLE RATE time, i.e.
                // in 8 / 16 000 Hz = 500 µs. After that there is 20 ms to queue the next.
                dma_future.await;
                mem::swap(&mut back_buffer, &mut front_buffer);
                prev_instant = end_of_decode;
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
    }
}

type MyUsbDriver = Driver<'static, USB>;
type MyUsbDevice = UsbDevice<'static, MyUsbDriver>;

#[embassy_executor::task]
async fn usb_task(mut usb: MyUsbDevice) -> ! {
    usb.run().await
}
