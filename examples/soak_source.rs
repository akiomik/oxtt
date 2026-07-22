//! Deterministic stereo JACK tone source for Raspberry Pi soak tests.

use std::env;
use std::error::Error;
use std::f32::consts::TAU;
use std::io;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use jack::{AudioOut, Client, ClientOptions, Control, Port, ProcessScope};

const CLIENT_NAME: &str = "soak-source";
const FREQUENCY_HZ: f32 = 997.0;
const AMPLITUDE: f32 = 0.05;

struct Notifications {
    shutdown: Arc<AtomicBool>,
}

impl jack::NotificationHandler for Notifications {
    unsafe fn shutdown(&mut self, _status: jack::ClientStatus, _reason: &str) {
        self.shutdown.store(true, Ordering::Release);
    }
}

struct ToneSource {
    output_l: Port<AudioOut>,
    output_r: Port<AudioOut>,
    waveform: Box<[f32]>,
    waveform_index: usize,
    frames_remaining: u64,
    completed: Arc<AtomicBool>,
}

impl jack::ProcessHandler for ToneSource {
    #[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
    fn process(&mut self, _: &Client, ps: &ProcessScope) -> Control {
        let output_l = self.output_l.as_mut_slice(ps);
        let output_r = self.output_r.as_mut_slice(ps);

        for (left, right) in output_l.iter_mut().zip(output_r.iter_mut()) {
            if self.frames_remaining == 0 {
                *left = 0.0;
                *right = 0.0;
                continue;
            }

            let sample = self.waveform[self.waveform_index];
            *left = sample;
            *right = sample;

            self.waveform_index += 1;
            if self.waveform_index == self.waveform.len() {
                self.waveform_index = 0;
            }
            self.frames_remaining -= 1;
        }

        if self.frames_remaining == 0 {
            self.completed.store(true, Ordering::Release);
        }
        Control::Continue
    }
}

#[allow(clippy::disallowed_macros)] // Startup and error reporting are outside the JACK callback.
fn main() -> ExitCode {
    let duration_seconds = match parse_duration_seconds() {
        Ok(seconds) => seconds,
        Err(message) => {
            eprintln!("soak-source: {message}");
            return ExitCode::from(2);
        }
    };

    match run(duration_seconds) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("soak-source: {error}");
            ExitCode::FAILURE
        }
    }
}

fn parse_duration_seconds() -> Result<u64, String> {
    let mut arguments = env::args_os();
    let _program = arguments.next();
    let Some(flag) = arguments.next() else {
        return Err("usage: soak_source --duration SECONDS".to_owned());
    };
    if flag != "--duration" {
        return Err("usage: soak_source --duration SECONDS".to_owned());
    }
    let Some(value) = arguments.next() else {
        return Err("usage: soak_source --duration SECONDS".to_owned());
    };
    if arguments.next().is_some() {
        return Err("usage: soak_source --duration SECONDS".to_owned());
    }

    let value = value
        .into_string()
        .map_err(|_| "duration must be valid UTF-8".to_owned())?;
    let seconds = value
        .parse::<u64>()
        .map_err(|_| "duration must be a positive integer".to_owned())?;
    if seconds == 0 {
        return Err("duration must be a positive integer".to_owned());
    }
    Ok(seconds)
}

fn run(duration_seconds: u64) -> Result<(), Box<dyn Error>> {
    let (client, _status) = Client::new(CLIENT_NAME, ClientOptions::default())?;
    let sample_rate = client.sample_rate();
    let sample_rate_usize = usize::try_from(sample_rate)
        .map_err(|_| io::Error::other("JACK sample rate does not fit usize"))?;
    let frames_remaining = duration_seconds
        .checked_mul(u64::from(sample_rate))
        .ok_or_else(|| io::Error::other("requested duration is too long"))?;

    #[allow(clippy::cast_precision_loss)]
    let phase_increment = TAU * FREQUENCY_HZ / sample_rate as f32;
    let waveform = (0..sample_rate_usize)
        .map(|frame| {
            #[allow(clippy::cast_precision_loss)]
            let phase = phase_increment * frame as f32;
            AMPLITUDE * phase.sin()
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();

    let output_l = client.register_port("out_1", AudioOut::default())?;
    let output_r = client.register_port("out_2", AudioOut::default())?;
    let completed = Arc::new(AtomicBool::new(false));
    let shutdown = Arc::new(AtomicBool::new(false));
    let active_client = client.activate_async(
        Notifications {
            shutdown: Arc::clone(&shutdown),
        },
        ToneSource {
            output_l,
            output_r,
            waveform,
            waveform_index: 0,
            frames_remaining,
            completed: Arc::clone(&completed),
        },
    )?;

    while !completed.load(Ordering::Acquire) && !shutdown.load(Ordering::Acquire) {
        #[allow(clippy::disallowed_methods)] // This is the non-real-time control thread.
        thread::sleep(Duration::from_millis(5));
    }
    active_client.deactivate()?;

    if shutdown.load(Ordering::Acquire) {
        return Err(io::Error::other("JACK server shut down").into());
    }
    Ok(())
}
