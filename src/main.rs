use evdev::{Device, EventType, InputEvent, InputEventKind, Key, LedType};
use serde::Deserialize;
use std::io::prelude::*;
use std::{env, fs, io};

#[derive(Deserialize)]
struct Config {
    hardware: HardwareConfig,
    server: ServerConfig,
}

#[derive(Deserialize)]
struct HardwareConfig {
    name: String,
    blink_speed: u64,
    escape: Key,
}

#[derive(Deserialize)]
struct ServerConfig {
    port: u64,
    api_key: String,
}

fn list_devices() {
    let mut devices = evdev::enumerate().map(|t| t.1).collect::<Vec<_>>();
    devices.reverse();
    println!("Connected Devices:");
    println!("index, name, physical_path");
    for (i, device) in devices.iter().enumerate() {
        println!(
            "{}, {}, {}",
            i,
            device.name().unwrap_or("[Unknown]"),
            device.physical_path().unwrap_or("[Unknown]")
        );
    }
}

fn prompt_bool(prompt: &str, default: bool) -> bool {
    if default {
        print!("{} (Y/n)? ", prompt);
    } else {
        print!("{} (y/N)? ", prompt);
    }
    let _ = io::stdout().flush();
    let mut input = String::new();
    if io::stdin().read_line(&mut input).is_err() {
        return default;
    }
    input = input.trim().to_lowercase();
    if input.starts_with("y") {
        return true;
    }
    if input.starts_with("n") {
        return false;
    }
    default
}

fn main() {
    list_devices();

    let config_file_path = env::current_exe()
        .expect("unable to obtain executable directory")
        .parent()
        .expect("unable to obtain executable directory")
        .join("config.toml");
    println!(
        "Loading configuration file \"{}\".",
        config_file_path.display()
    );
    let config_data =
        fs::read_to_string(config_file_path).expect("unable to read configuration file");
    let config: Config =
        toml::from_str(&config_data).expect("unable to deserialize configuration file");
    //let escapeKey = Key::from_str(config.hardware.escape);

    println!("Searching for device \"{}\".", config.hardware.name);
    let mut keyboard = evdev::enumerate()
        .find(|t| {
            if let Some(name) = t.1.name() {
                name == config.hardware.name
            } else {
                false
            }
        })
        .expect("unable to find device")
        .1;
    let grabbing = prompt_bool("Capture device inputs", false);
    if grabbing {
        keyboard.grab().expect("unable to grab device");
        println!("Capturing device inputs.");
        'event_loop: loop {
            match keyboard.fetch_events() {
                Ok(events) => {
                    for event in events {
                        println!("{event:?}");
                        if event.event_type() == EventType::KEY
                            && event.code() == config.hardware.escape.code()
                        {
                            break 'event_loop;
                        }
                    }
                }
                Err(error) => {
                    println!("Failed to fetch events: {error:?}");
                }
            }
        }
        keyboard.ungrab().expect("unable to ungrab device");
    } else {
        println!("Blinking the Keyboard LEDS...");
        for i in 0..5 {
            let on = i % 2 != 0;
            keyboard
                .send_events(&[InputEvent::new(
                    EventType::LED,
                    LedType::LED_NUML.0,
                    if on { i32::MAX } else { 0 },
                )])
                .unwrap();
            std::thread::sleep(std::time::Duration::from_secs(config.hardware.blink_speed));
        }
    }
}
