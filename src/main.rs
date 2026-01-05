use std::fs::{self, File};
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{fmt, io, mem, process, slice, thread};

use argh::FromArgs;
use nix::sys::ioctl::ioctl_param_type;
use nix::{ioctl_read, ioctl_write_int, ioctl_write_ptr};

// Force-feedback event type constants.
// <https://github.com/torvalds/linux/blob/9f4211bf7f811b653aa6acfb9aea38222436a458/include/uapi/linux/input-event-codes.h#L47>
const EV_FF: u16 = 0x15;
const FF_RUMBLE: u16 = 0x50;

/// Length of the force-feedback capabilities array.
const FEATURES_LEN: usize = (libc::FF_CNT - 1) / (8 * mem::size_of::<libc::c_ulong>()) + 1;

/// Force-feedback device control utility.
#[derive(FromArgs, Default)]
pub struct Cli {
    /// duration of each vibration in milliseconds
    #[argh(positional)]
    length: u16,
    /// time between vibrations in milliseconds
    #[argh(positional)]
    interval: u16,
    /// number of vibrations
    #[argh(positional)]
    count: u16,
    /// force-feedback device path
    #[argh(option)]
    device_path: Option<PathBuf>,
}

fn main() {
    let cli: Cli = argh::from_env();

    let mut vibrator = match cli.device_path {
        Some(device_path) => match Vibrator::new(&device_path) {
            Ok(vibrator) => vibrator,
            Err(err) => {
                eprintln!("Error: Could not open device {device_path:?}: {err}");
                process::exit(1);
            },
        },
        None => match Vibrator::search() {
            Some(vibrator) => vibrator,
            None => {
                eprintln!("Error: No force-feedback device found");
                process::exit(1);
            },
        },
    };

    match vibrator.vibrate(cli.length, cli.interval, cli.count) {
        Ok(()) => (),
        Err(err) => {
            eprintln!("Error: Failed to play rumble effect: {err}");
        },
    }
}

/// Force-feedback interface.
pub struct Vibrator {
    device: File,
}

impl Vibrator {
    fn new(device_path: &Path) -> Result<Self, io::Error> {
        Ok(Self { device: File::options().append(true).open(device_path)? })
    }

    /// Search /dev/input for a device with vibration capabilities.
    fn search() -> Option<Self> {
        let input_dir = fs::read_dir("/dev/input")
            .inspect_err(|err| eprintln!("Warn: Could not read /dev/input: {err}"))
            .ok()?;

        for entry in input_dir {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    eprintln!("Warn: Could not access /dev/input entry: {err}");
                    continue;
                },
            };

            // Ignore files other than `/dev/input/event*`.
            let path = entry.path();
            let file_name = path.file_name().and_then(|name| name.to_str());
            if file_name.is_none_or(|name| !name.starts_with("event")) {
                continue;
            }

            // Open file to get its FD.
            let file = match File::options().append(true).open(&path) {
                Ok(file) => file,
                Err(err) => {
                    eprintln!("Failed to open device file {path:?}: {err}");
                    continue;
                },
            };
            let fd = file.as_raw_fd();

            // Query for device capabilities.
            let mut data = [0; FEATURES_LEN];
            if let Err(err) = unsafe { features(fd, &mut data) } {
                eprintln!("Warn: Failed to query force-feedback features of {path:?}: {err}");
                continue;
            }

            // Check if the device has the rumble capability.
            let index = FF_RUMBLE as usize / (8 * mem::size_of::<libc::c_ulong>());
            let offset = FF_RUMBLE as usize % (8 * mem::size_of::<libc::c_ulong>());
            if index < FEATURES_LEN && data[index] >> offset & 1 != 0 {
                return Some(Self { device: file });
            }
        }

        None
    }

    /// Stop vibration and remove effect from device.
    fn stop(&mut self, effect_id: u16) -> Result<(), String> {
        let fd = self.device.as_raw_fd();
        match unsafe { remove_effect(fd, effect_id as ioctl_param_type) } {
            Ok(_) => Ok(()),
            Err(_) => {
                let last_error = io::Error::last_os_error();
                let msg = format!("Warn: Failed to remove rumble effect: {last_error}");
                Err(msg)
            },
        }
    }

    /// Play a rumble effect.
    ///
    /// This will block until the effect has finished playing.
    ///
    /// Unsafe wrapper for the purpose of error handling.
    /// Use [`Self::vibrate`] instead.
    fn vibrate(&mut self, length: u16, interval: u16, count: u16) -> Result<(), String> {
        // Ignore without rumble device access.
        let mut effect = Effect {
            effect_type: FF_RUMBLE,
            id: -1,
            direction: 0,
            trigger: Trigger { interval: 0, button: 0 },
            replay: Replay { length, delay: interval },
            data: EffectData { rumble: Rumble { strong: u16::MAX, weak: 0 } },
        };

        // Upload effect to the device.
        let result = unsafe { upload_effect(self.device.as_raw_fd(), &mut effect as *const _) };
        if let Err(err) = result {
            return Err(format!("Failed to upload rumble effect: {err}"));
        }

        // Ensure effect ID was set to a positive integer.
        let effect_id = match u16::try_from(effect.id) {
            Ok(effect_id) => effect_id,
            Err(_) => return Err(format!("Invalid rumble effect ID: {effect:?}")),
        };

        // Play effect `count` times.
        let play = libc::input_event {
            time: libc::timeval { tv_sec: 0, tv_usec: 0 },
            code: effect_id,
            value: count as i32,
            type_: EV_FF,
        };
        let play_ptr = (&play as *const libc::input_event).cast();
        let play_size = mem::size_of::<libc::input_event>();
        let play_data = unsafe { slice::from_raw_parts(play_ptr, play_size) };
        self.device
            .write(play_data)
            .map_err(|err| format!("Failed to submit rumble event: {err}"))?;

        // Remove effect after it finished playing.
        let duration = Duration::from_millis(((length + interval) * count) as u64);
        thread::sleep(duration);
        self.stop(effect_id)?;

        Ok(())
    }
}

ioctl_read!(features, b'E', 0x20 + EV_FF, [libc::c_ulong; FEATURES_LEN]);
ioctl_write_ptr!(upload_effect, b'E', 0x80, Effect);
ioctl_write_int!(remove_effect, b'E', 0x81);

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct Effect {
    effect_type: u16,
    id: i16,
    direction: u16,
    trigger: Trigger,
    replay: Replay,
    data: EffectData,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
struct Trigger {
    interval: u16,
    button: u16,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
struct Replay {
    length: u16,
    delay: u16,
}

#[repr(C)]
#[derive(Copy, Clone)]
union EffectData {
    rumble: Rumble,
    #[cfg(target_pointer_width = "64")]
    padding: [u64; 4],
    #[cfg(target_pointer_width = "32")]
    padding: [u32; 7],
}

impl fmt::Debug for EffectData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        unsafe { self.padding.fmt(f) }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
struct Rumble {
    strong: u16,
    weak: u16,
}
