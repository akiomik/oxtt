//! Deterministic stereo JACK recorder for Raspberry Pi soak tests.
//!
//! This intentionally avoids an OS-provided JACK recording utility. The JACK
//! callback only converts samples and writes to a lock-free ring buffer; a
//! normal-priority thread writes the WAV file and makes every loss explicit.

use std::env;
use std::error::Error;
use std::fs::File;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use jack::{
    AudioIn, Client, ClientOptions, Control, Port, ProcessScope, RingBuffer, RingBufferReader,
    RingBufferWriter,
};

const CLIENT_NAME: &str = "soak-recorder";
const FRAME_BYTES: usize = 4;
const RING_BUFFER_BYTES: usize = 1 << 20;
const DISK_BUFFER_BYTES: usize = 64 << 10;

struct Arguments {
    duration_seconds: u64,
    output: PathBuf,
}

struct Notifications {
    shutdown: Arc<AtomicBool>,
}

impl jack::NotificationHandler for Notifications {
    unsafe fn shutdown(&mut self, _status: jack::ClientStatus, _reason: &str) {
        self.shutdown.store(true, Ordering::Release);
    }
}

struct Recorder {
    input_l: Port<AudioIn>,
    input_r: Port<AudioIn>,
    writer: RingBufferWriter,
    frames_remaining: u64,
    queued_frames: Arc<AtomicU64>,
    dropped_frames: Arc<AtomicU64>,
    completed: Arc<AtomicBool>,
}

impl jack::ProcessHandler for Recorder {
    #[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
    fn process(&mut self, _: &Client, ps: &ProcessScope) -> Control {
        let input_l = self.input_l.as_slice(ps);
        let input_r = self.input_r.as_slice(ps);
        let available_frames = input_l.len().min(input_r.len());
        let wanted_frames = usize::try_from(self.frames_remaining)
            .unwrap_or(available_frames)
            .min(available_frames);
        let writable_frames = (self.writer.space() / FRAME_BYTES).min(wanted_frames);

        let (first, second) = self.writer.get_vector();
        let mut source_index = 0;
        let first_bytes = first
            .len()
            .min((writable_frames - source_index) * FRAME_BYTES);
        for frame in first[..first_bytes].chunks_exact_mut(FRAME_BYTES) {
            encode_frame(frame, input_l[source_index], input_r[source_index]);
            source_index += 1;
        }
        let second_bytes = second
            .len()
            .min((writable_frames - source_index) * FRAME_BYTES);
        for frame in second[..second_bytes].chunks_exact_mut(FRAME_BYTES) {
            encode_frame(frame, input_l[source_index], input_r[source_index]);
            source_index += 1;
        }
        self.writer.advance(writable_frames * FRAME_BYTES);
        self.queued_frames.fetch_add(
            u64::try_from(writable_frames).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );

        let dropped = wanted_frames - writable_frames;
        self.dropped_frames.fetch_add(
            u64::try_from(dropped).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
        self.frames_remaining -= u64::try_from(wanted_frames).unwrap_or(u64::MAX);
        if self.frames_remaining == 0 {
            self.completed.store(true, Ordering::Release);
        }
        Control::Continue
    }
}

#[allow(clippy::disallowed_macros)] // Startup and error reporting are outside the JACK callback.
fn main() -> ExitCode {
    let arguments = match parse_arguments() {
        Ok(arguments) => arguments,
        Err(message) => {
            eprintln!("soak-recorder: {message}");
            return ExitCode::from(2);
        }
    };

    match run(arguments) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("soak-recorder: {error}");
            ExitCode::FAILURE
        }
    }
}

fn parse_arguments() -> Result<Arguments, String> {
    let mut arguments = env::args_os();
    let _program = arguments.next();
    let Some(duration_flag) = arguments.next() else {
        return Err(usage());
    };
    let Some(duration_value) = arguments.next() else {
        return Err(usage());
    };
    let Some(output_flag) = arguments.next() else {
        return Err(usage());
    };
    let Some(output) = arguments.next() else {
        return Err(usage());
    };
    if duration_flag != "--duration" || output_flag != "--output" || arguments.next().is_some() {
        return Err(usage());
    }
    let seconds = duration_value
        .into_string()
        .map_err(|_| "duration must be valid UTF-8".to_owned())?
        .parse::<u64>()
        .map_err(|_| "duration must be a positive integer".to_owned())?;
    if seconds == 0 {
        return Err("duration must be a positive integer".to_owned());
    }
    Ok(Arguments {
        duration_seconds: seconds,
        output: PathBuf::from(output),
    })
}

fn usage() -> String {
    "usage: soak_recorder --duration SECONDS --output PATH".to_owned()
}

#[allow(clippy::disallowed_macros, clippy::disallowed_methods)] // This is the non-real-time control thread.
fn run(arguments: Arguments) -> Result<(), Box<dyn Error>> {
    let (client, _status) = Client::new(CLIENT_NAME, ClientOptions::default())?;
    let sample_rate = client.sample_rate();
    let target_frames = arguments
        .duration_seconds
        .checked_mul(u64::from(sample_rate))
        .ok_or_else(|| io::Error::other("requested duration is too long"))?;
    let target_data_bytes = target_frames
        .checked_mul(u64::try_from(FRAME_BYTES)?)
        .ok_or_else(|| io::Error::other("requested duration is too long"))?;
    let target_data_bytes = u32::try_from(target_data_bytes)
        .map_err(|_| io::Error::other("requested WAV would exceed 4 GiB"))?;

    let mut ring_buffer = RingBuffer::new(RING_BUFFER_BYTES)?;
    ring_buffer.mlock();
    let (reader, writer) = ring_buffer.into_reader_writer();
    let producer_done = Arc::new(AtomicBool::new(false));
    let writer_done = Arc::clone(&producer_done);
    let output = arguments.output;
    let writer_thread = thread::spawn(move || {
        write_wav(output, sample_rate, target_data_bytes, reader, writer_done)
    });

    let input_l = client.register_port("in_1", AudioIn::default())?;
    let input_r = client.register_port("in_2", AudioIn::default())?;
    let completed = Arc::new(AtomicBool::new(false));
    let shutdown = Arc::new(AtomicBool::new(false));
    let queued_frames = Arc::new(AtomicU64::new(0));
    let dropped_frames = Arc::new(AtomicU64::new(0));
    let active_client = client.activate_async(
        Notifications {
            shutdown: Arc::clone(&shutdown),
        },
        Recorder {
            input_l,
            input_r,
            writer,
            frames_remaining: target_frames,
            queued_frames: Arc::clone(&queued_frames),
            dropped_frames: Arc::clone(&dropped_frames),
            completed: Arc::clone(&completed),
        },
    )?;

    while !completed.load(Ordering::Acquire) && !shutdown.load(Ordering::Acquire) {
        #[allow(clippy::disallowed_methods)] // This is the non-real-time control thread.
        thread::sleep(Duration::from_millis(5));
    }
    active_client.deactivate()?;
    producer_done.store(true, Ordering::Release);

    let written_frames = writer_thread
        .join()
        .map_err(|_| io::Error::other("WAV writer thread panicked"))??;
    if shutdown.load(Ordering::Acquire) {
        return Err(io::Error::other("JACK server shut down").into());
    }
    let queued_frames = queued_frames.load(Ordering::Relaxed);
    let dropped_frames = dropped_frames.load(Ordering::Relaxed);
    if dropped_frames != 0 || queued_frames != target_frames || written_frames != target_frames {
        return Err(io::Error::other(format!(
            "recording loss: queued_frames={queued_frames} written_frames={written_frames} dropped_frames={dropped_frames} target_frames={target_frames}"
        ))
        .into());
    }

    println!("queued_frames={queued_frames}");
    println!("written_frames={written_frames}");
    println!("dropped_frames={dropped_frames}");
    Ok(())
}

#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
fn sample_to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16
}

#[allow(clippy::indexing_slicing)]
fn encode_frame(destination: &mut [u8], left: f32, right: f32) {
    let left = sample_to_i16(left).to_le_bytes();
    let right = sample_to_i16(right).to_le_bytes();
    destination[0] = left[0];
    destination[1] = left[1];
    destination[2] = right[0];
    destination[3] = right[1];
}

#[allow(
    clippy::arithmetic_side_effects,
    clippy::disallowed_macros,
    clippy::disallowed_methods,
    clippy::indexing_slicing,
    clippy::needless_pass_by_value
)] // This is the normal-priority disk-writer thread, never the JACK callback.
fn write_wav(
    output: PathBuf,
    sample_rate: u32,
    target_data_bytes: u32,
    mut reader: RingBufferReader,
    producer_done: Arc<AtomicBool>,
) -> io::Result<u64> {
    let mut file = File::create(output)?;
    write_wav_header(&mut file, sample_rate, target_data_bytes)?;
    let mut buffer = vec![0_u8; DISK_BUFFER_BYTES];
    let mut written_bytes = 0_u64;
    loop {
        let count = reader.read_buffer(&mut buffer);
        if count != 0 {
            file.write_all(&buffer[..count])?;
            written_bytes +=
                u64::try_from(count).map_err(|_| io::Error::other("write count overflow"))?;
            continue;
        }
        if producer_done.load(Ordering::Acquire) {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    file.flush()?;
    if written_bytes
        % u64::try_from(FRAME_BYTES).map_err(|_| io::Error::other("frame size overflow"))?
        != 0
    {
        return Err(io::Error::other("WAV data is not frame-aligned"));
    }
    Ok(written_bytes
        / u64::try_from(FRAME_BYTES).map_err(|_| io::Error::other("frame size overflow"))?)
}

fn write_wav_header(file: &mut File, sample_rate: u32, data_bytes: u32) -> io::Result<()> {
    let riff_size = 36_u32
        .checked_add(data_bytes)
        .ok_or_else(|| io::Error::other("WAV size overflow"))?;
    let byte_rate = sample_rate
        .checked_mul(
            u32::try_from(FRAME_BYTES).map_err(|_| io::Error::other("frame size overflow"))?,
        )
        .ok_or_else(|| io::Error::other("WAV byte rate overflow"))?;
    file.write_all(b"RIFF")?;
    file.write_all(&riff_size.to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16_u32.to_le_bytes())?;
    file.write_all(&1_u16.to_le_bytes())?;
    file.write_all(&2_u16.to_le_bytes())?;
    file.write_all(&sample_rate.to_le_bytes())?;
    file.write_all(&byte_rate.to_le_bytes())?;
    file.write_all(
        &u16::try_from(FRAME_BYTES)
            .map_err(|_| io::Error::other("frame size overflow"))?
            .to_le_bytes(),
    )?;
    file.write_all(&16_u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&data_bytes.to_le_bytes())?;
    Ok(())
}
