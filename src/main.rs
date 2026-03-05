use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::thread;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use evdev::{AbsoluteAxisType, Device, InputEventKind, Key, RelativeAxisType};
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};
use serde::Deserialize;

const DEFAULT_DOWN_SOUND: &str = "click_down.wav";
const DEFAULT_UP_SOUND: &str = "click_up.wav";

#[derive(Parser, Debug)]
#[command(name = "mouse-sounds")]
#[command(about = "Play sounds on global mouse down/up events")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Run {
        #[arg(long)]
        config: Option<PathBuf>,
    },
    Check {
        #[arg(long)]
        config: Option<PathBuf>,
    },
}

#[derive(Debug, Clone)]
struct RuntimeSettings {
    down_path: PathBuf,
    up_path: PathBuf,
    event_path: Option<PathBuf>,
    all_buttons: bool,
}

#[derive(Debug, Default, Deserialize)]
struct ConfigFile {
    #[serde(default)]
    sounds: SoundsSection,
    #[serde(default)]
    device: DeviceSection,
    #[serde(default)]
    behavior: BehaviorSection,
}

#[derive(Debug, Default, Deserialize)]
struct SoundsSection {
    down: Option<String>,
    up: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct DeviceSection {
    event_path: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct BehaviorSection {
    all_buttons: Option<bool>,
}

struct AudioPlayer {
    _stream: OutputStream,
    stream_handle: OutputStreamHandle,
    down_sound: Arc<[u8]>,
    up_sound: Arc<[u8]>,
}

struct InputDevice {
    path: PathBuf,
    name: String,
    device: Device,
}

#[derive(Debug, Clone, Copy)]
enum MouseSignal {
    Down,
    Up,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Check { config }) => check_command(config.as_deref()),
        Some(Commands::Run { config }) => run_command(config.as_deref()),
        None => run_command(None),
    }
}

fn check_command(config_path: Option<&Path>) -> Result<()> {
    let settings = load_settings(config_path)?;
    validate_paths(&settings)?;
    validate_wav(&settings.down_path, "down")?;
    validate_wav(&settings.up_path, "up")?;
    let devices = open_input_devices(&settings)?;

    println!("check ok");
    println!("down: {}", settings.down_path.display());
    println!("up: {}", settings.up_path.display());
    println!("devices: {}", devices.len());
    for device in devices {
        println!("device: {} ({})", device.path.display(), device.name);
    }
    Ok(())
}

fn run_command(config_path: Option<&Path>) -> Result<()> {
    let settings = load_settings(config_path)?;
    validate_paths(&settings)?;

    let devices = open_input_devices(&settings)?;
    let player = AudioPlayer::new(&settings.down_path, &settings.up_path)?;
    let all_buttons = settings.all_buttons;
    let (sender, receiver) = mpsc::channel();

    eprintln!("down sound: {}", settings.down_path.display());
    eprintln!("up sound: {}", settings.up_path.display());
    eprintln!("all_buttons: {all_buttons}");
    eprintln!("input devices: {}", devices.len());
    for device in devices {
        eprintln!("input device: {} ({})", device.path.display(), device.name);
        spawn_device_listener(device, all_buttons, sender.clone());
    }
    drop(sender);

    while let Ok(signal) = receiver.recv() {
        let play_result = match signal {
            MouseSignal::Down => player.play_down(),
            MouseSignal::Up => player.play_up(),
        };

        if let Err(err) = play_result {
            eprintln!("failed to play sound: {err:#}");
        }
    }

    bail!("all mouse input listeners stopped")
}

impl AudioPlayer {
    fn new(down_path: &Path, up_path: &Path) -> Result<Self> {
        let down_sound = load_sound_bytes(down_path, "down")?;
        let up_sound = load_sound_bytes(up_path, "up")?;
        let (_stream, stream_handle) =
            OutputStream::try_default().context("failed to open default audio output")?;

        Ok(Self {
            _stream,
            stream_handle,
            down_sound,
            up_sound,
        })
    }

    fn play_down(&self) -> Result<()> {
        self.play_sound(self.down_sound.clone(), "down")
    }

    fn play_up(&self) -> Result<()> {
        self.play_sound(self.up_sound.clone(), "up")
    }

    fn play_sound(&self, bytes: Arc<[u8]>, label: &str) -> Result<()> {
        let sink = Sink::try_new(&self.stream_handle).context("failed to create sink")?;
        let source =
            Decoder::new(Cursor::new(bytes)).with_context(|| format!("invalid {label} sound"))?;
        sink.append(source);
        sink.detach();
        Ok(())
    }
}

fn load_settings(config_path: Option<&Path>) -> Result<RuntimeSettings> {
    let cwd = std::env::current_dir().context("failed to get current directory")?;
    let mut settings = RuntimeSettings {
        down_path: cwd.join(DEFAULT_DOWN_SOUND),
        up_path: cwd.join(DEFAULT_UP_SOUND),
        event_path: None,
        all_buttons: true,
    };

    if let Some(config_path) = config_path {
        let config_raw = fs::read_to_string(config_path)
            .with_context(|| format!("failed to read config file: {}", config_path.display()))?;
        let config: ConfigFile = toml::from_str(&config_raw)
            .with_context(|| format!("failed to parse config file: {}", config_path.display()))?;
        let config_base = config_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();

        if let Some(down) = config.sounds.down.as_deref() {
            let down = down.trim();
            if !down.is_empty() {
                settings.down_path = resolve_config_path(&config_base, down);
            }
        }

        if let Some(up) = config.sounds.up.as_deref() {
            let up = up.trim();
            if !up.is_empty() {
                settings.up_path = resolve_config_path(&config_base, up);
            }
        }

        if let Some(event_path) = config.device.event_path.as_deref() {
            let event_path = event_path.trim();
            if !event_path.is_empty() {
                settings.event_path = Some(resolve_config_path(&config_base, event_path));
            }
        }

        if let Some(all_buttons) = config.behavior.all_buttons {
            settings.all_buttons = all_buttons;
        }
    }

    Ok(settings)
}

fn resolve_config_path(config_base: &Path, path_value: &str) -> PathBuf {
    let candidate = PathBuf::from(path_value);
    if candidate.is_absolute() {
        candidate
    } else {
        config_base.join(candidate)
    }
}

fn validate_paths(settings: &RuntimeSettings) -> Result<()> {
    validate_sound_path(&settings.down_path, "down")?;
    validate_sound_path(&settings.up_path, "up")?;
    Ok(())
}

fn validate_sound_path(path: &Path, label: &str) -> Result<()> {
    if !path.exists() {
        bail!("{label} sound file does not exist: {}", path.display());
    }

    if !path.is_file() {
        bail!("{label} sound path is not a file: {}", path.display());
    }

    fs::File::open(path)
        .with_context(|| format!("{label} sound file is not readable: {}", path.display()))?;
    Ok(())
}

fn validate_wav(path: &Path, label: &str) -> Result<()> {
    let bytes = fs::read(path)
        .with_context(|| format!("failed to read {label} sound: {}", path.display()))?;
    Decoder::new(Cursor::new(bytes)).with_context(|| {
        format!(
            "{label} sound is not a valid audio file: {}",
            path.display()
        )
    })?;
    Ok(())
}

fn load_sound_bytes(path: &Path, label: &str) -> Result<Arc<[u8]>> {
    let bytes = fs::read(path)
        .with_context(|| format!("failed to read {label} sound: {}", path.display()))?;
    let bytes: Arc<[u8]> = bytes.into();
    Decoder::new(Cursor::new(bytes.clone())).with_context(|| {
        format!(
            "{label} sound is not a valid audio file: {}",
            path.display()
        )
    })?;
    Ok(bytes)
}

fn spawn_device_listener(
    mut input_device: InputDevice,
    all_buttons: bool,
    sender: mpsc::Sender<MouseSignal>,
) {
    thread::spawn(move || {
        loop {
            let events = match input_device.device.fetch_events() {
                Ok(events) => events,
                Err(err) => {
                    eprintln!(
                        "mouse listener stopped for {} ({}): {err:#}",
                        input_device.path.display(),
                        input_device.name
                    );
                    break;
                }
            };

            for event in events {
                if let InputEventKind::Key(key) = event.kind() {
                    if !should_handle_button(key, all_buttons) {
                        continue;
                    }

                    let signal = match event.value() {
                        1 => Some(MouseSignal::Down),
                        0 => Some(MouseSignal::Up),
                        _ => None,
                    };

                    if let Some(signal) = signal {
                        if sender.send(signal).is_err() {
                            return;
                        }
                    }
                }
            }
        }
    });
}

fn open_input_devices(settings: &RuntimeSettings) -> Result<Vec<InputDevice>> {
    if let Some(path) = settings.event_path.as_deref() {
        return Ok(vec![open_mouse_device(path)?]);
    }

    auto_select_mouse_devices()
}

fn open_mouse_device(path: &Path) -> Result<InputDevice> {
    let device = Device::open(path)
        .with_context(|| format!("failed to open input device: {}", path.display()))?;
    if !device_is_mouse_device(&device) {
        bail!(
            "configured device does not look like a mouse pointer with buttons: {}",
            path.display()
        );
    }

    let name = device.name().unwrap_or("<unnamed>").to_string();
    Ok(InputDevice {
        path: path.to_path_buf(),
        name,
        device,
    })
}

fn auto_select_mouse_devices() -> Result<Vec<InputDevice>> {
    let mut candidates = Vec::new();
    let mut devices = Vec::new();
    let entries = fs::read_dir("/dev/input").context("failed to read /dev/input")?;

    for entry in entries {
        let entry = entry.context("failed to read /dev/input entry")?;
        let path = entry.path();
        if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
            if name.starts_with("event") {
                candidates.push(path);
            }
        }
    }

    candidates.sort();

    for candidate in candidates {
        let device = match Device::open(&candidate) {
            Ok(device) => device,
            Err(_) => continue,
        };

        if device_is_mouse_device(&device) {
            let name = device.name().unwrap_or("<unnamed>").to_string();
            devices.push(InputDevice {
                path: candidate,
                name,
                device,
            });
        }
    }

    if devices.is_empty() {
        bail!("no readable mouse input device found under /dev/input/event*");
    }

    Ok(devices)
}

fn device_is_mouse_device(device: &Device) -> bool {
    has_mouse_buttons(device) && has_pointer_axes(device)
}

fn has_mouse_buttons(device: &Device) -> bool {
    let Some(keys) = device.supported_keys() else {
        return false;
    };

    mouse_button_keys().iter().any(|key| keys.contains(*key))
}

fn has_pointer_axes(device: &Device) -> bool {
    let has_relative_xy = device.supported_relative_axes().is_some_and(|axes| {
        axes.contains(RelativeAxisType::REL_X) && axes.contains(RelativeAxisType::REL_Y)
    });

    let has_absolute_xy = device.supported_absolute_axes().is_some_and(|axes| {
        axes.contains(AbsoluteAxisType::ABS_X) && axes.contains(AbsoluteAxisType::ABS_Y)
    });

    has_relative_xy || has_absolute_xy
}

fn should_handle_button(key: Key, all_buttons: bool) -> bool {
    if all_buttons {
        mouse_button_keys().contains(&key)
    } else {
        key == Key::BTN_LEFT
    }
}

fn mouse_button_keys() -> [Key; 8] {
    [
        Key::BTN_LEFT,
        Key::BTN_RIGHT,
        Key::BTN_MIDDLE,
        Key::BTN_SIDE,
        Key::BTN_EXTRA,
        Key::BTN_FORWARD,
        Key::BTN_BACK,
        Key::BTN_TASK,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_only_left_when_all_buttons_disabled() {
        assert!(should_handle_button(Key::BTN_LEFT, false));
        assert!(!should_handle_button(Key::BTN_RIGHT, false));
        assert!(!should_handle_button(Key::KEY_A, false));
    }

    #[test]
    fn handles_all_mouse_buttons_when_enabled() {
        for key in mouse_button_keys() {
            assert!(should_handle_button(key, true));
        }
    }

    #[test]
    fn ignores_non_mouse_keys_when_enabled() {
        assert!(!should_handle_button(Key::KEY_A, true));
    }
}
