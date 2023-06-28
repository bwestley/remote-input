use bus::{Bus, BusReader};
use evdev::{Device, EventType, InputEvent, Key, LedType};
use serde::{Deserialize, Serialize};
use std::io::{prelude::*, BufReader};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{fs, thread};
mod as_hex;
mod thread_pool;

/// Holds configuration values read from config.toml.
#[derive(Serialize, Deserialize, Clone)]
struct Config {
    hardware: HardwareConfig,
    server: ServerConfig,
}

/// Holds server configuration values read from config.toml.
#[derive(Serialize, Deserialize, Clone)]
struct HardwareConfig {
    name: String,
    led_speed_millis: u64,
    escape: Key,
    pause: Key,
}

/// Holds server configuration values read from config.toml.
#[derive(Serialize, Deserialize, Clone)]
struct ServerConfig {
    address: String,
    api_key: String,
}

/// Holds information about an input event. Serialized using postcard and sent to clients.
/// Enum values can be found in https://github.com/torvalds/linux/blob/master/include/uapi/linux/input-event-codes.h
/// Fields:
/// - `timestamp`: a `std::time::SystemTime` associated with the event
/// - `event_type`: the raw type (e.g., a key press)
/// - `code`: the raw code (e.g., corresponding to a certain key)
/// - `value`: the raw value (e.g., 1 for a key press and 0 for a key release)
#[derive(Serialize)]
struct InputEventWrapper {
    timestamp: std::time::SystemTime,
    event_type: u16,
    code: u16,
    value: i32,
}

impl From<InputEvent> for InputEventWrapper {
    fn from(input_event: InputEvent) -> Self {
        Self {
            timestamp: input_event.timestamp(),
            event_type: input_event.event_type().0,
            code: input_event.code(),
            value: input_event.value(),
        }
    }
}

/// Iterate over enumerated devices and print information.
fn list_devices() {
    println!("[List Devices] Connected Devices:");
    println!("[List Devices] path, name, physical_path");
    for (path, device) in evdev::enumerate() {
        println!(
            "[List Devices] {}, {}, {}",
            path.display(),
            device.name().unwrap_or("[Unknown]"),
            device.physical_path().unwrap_or("[Unknown]")
        );
    }
}

/// Finds the `Device` from `evdev::enumerate()` with the name `device_name`.
fn find_device(device_name: &String) -> Option<Device> {
    Some(
        evdev::enumerate()
            .find(|enumerated_device| {
                if let Some(name) = enumerated_device.1.name() {
                    name == device_name
                } else {
                    false
                }
            })?
            .1,
    )
}

/// Listens for input events from the device with `device_name`, serializes them, and sends them through `event_bus`.
/// The device is grabbed, preventing input events from propagating.
/// When a key with the code `escape_code` is pressed, grab or ungrab the device.
/// When a key with the code `pause_code` is pressed, discard events until it is pressed again.
///
/// Events are converted into [`InputEventWrapper`] before being serialized by [`postcard`] and encoded by COBS.
/// Serialized events are transmitted over `event_bus` in `[u8; 64]` buffer with a [`usize`] indexing the last byte of the data (which should always be 0x00).
/// Bytes in the buffer after this index are undefined garbage.
/// Use `event.0[0..event.1]` to extract the serialized event slice from the buffer.
fn device_listener(
    device_name: &String,
    escape_code: u16,
    pause_code: u16,
    event_bus: Arc<Mutex<Bus<([u8; 64], usize)>>>,
) {
    println!(
        "[Device Listener] Searching for device \"{}\".",
        device_name
    );
    let mut keyboard = find_device(device_name).expect("unable to find device");

    let mut grabbed = false; // Should reflect keyboard.raw.grabbed
    let mut grab_target = true; // The intended state of keyboard.raw.grabbed as controlled by pressing `escape_code`.
    let mut pause = true; // Events are discarded when pause is true.
    let mut pause_target = false; // The intended state of pause as controlled by pressing `pause_code`.

    let mut event_buffer = [0u8; 64]; // Holds serialized events. Size is fixed so that it can be sent through `event_bus`.

    println!("[Device Listener] Listening for events.");
    loop {
        // Grab and ungrab device as needed to reach `grab_target`.
        // If that fails, prevent retrying by setting `grab_target` to `grabbed`.
        // Send LED_SCROLLL events to display `grabbed`.
        if grabbed != grab_target {
            if grab_target {
                match keyboard.grab() {
                    Ok(_) => {
                        println!("[Device Listener] Grabbed device.");
                        if let Err(error) = keyboard.send_events(&[InputEvent::new(
                            EventType::LED,
                            LedType::LED_SCROLLL.0,
                            1,
                        )]) {
                            println!("[Device Listener] Unable to set LED_SCROLLL: {error}.")
                        };
                        grabbed = true;
                    }
                    Err(error) => {
                        println!("[Device Listener] Unable to grab device: {error}.");
                        grab_target = false;
                    }
                }
            } else {
                match keyboard.ungrab() {
                    Ok(_) => {
                        println!("[Device Listener] Ungrabbed device.");
                        if let Err(error) = keyboard.send_events(&[InputEvent::new(
                            EventType::LED,
                            LedType::LED_SCROLLL.0,
                            0,
                        )]) {
                            println!("[Device Listener] Unable to reset LED_SCROLLL: {error}.")
                        };
                        grabbed = false;
                    }
                    Err(error) => {
                        println!("[Device Listener] Unable to ungrab device: {error}.");
                        grab_target = true;
                    }
                }
            }
        }

        if pause != pause_target {
            pause ^= true;
            println!(
                "[Device Listener] {} event transmission.",
                if pause { "Paused" } else { "Unpaused" }
            );
            if let Err(error) = keyboard.send_events(&[InputEvent::new(
                EventType::LED,
                LedType::LED_CAPSL.0,
                pause as i32,
            )]) {
                println!(
                    "[Device Listener] Unable to {} LED_CAPSL: {error}.",
                    if pause { "set" } else { "reset" }
                )
            };
        }

        // Process each input event in the kernel ring buffer.
        match keyboard.fetch_events() {
            Ok(events) => {
                // Acquire the transmitter of `event_bus`.
                // This will block if and while a new receiver is added when a TCP request is received.
                let mut transmitter = event_bus.lock().unwrap();
                for event in events {
                    // Ignore LED events, most are emitted from `blink_led`.
                    if event.event_type() == EventType::LED {
                        continue;
                    }

                    println!("[Device Listener] Event: {event:?}");

                    // Receive grab/ungrab and pause requests.
                    // Absorb all `escape_code` and `pause_code` key presses.
                    if event.event_type() == EventType::KEY {
                        if event.code() == escape_code {
                            grab_target ^= event.value() == 0;
                            continue;
                        }
                        if event.code() == pause_code {
                            if event.value() == 0 {
                                pause_target ^= true;
                            }
                            continue;
                        }
                    }

                    // Transmit serialized event to the bus.
                    if !pause && transmitter.rx_count() >= 1 {
                        match postcard::to_slice_cobs(
                            &InputEventWrapper::from(event),
                            &mut event_buffer,
                        ) {
                            Err(error) => {
                                println!("[Device Listener] Failed to serialize event: {error}.")
                            }
                            Ok(serialized_event) => {
                                let len = serialized_event.len();
                                println!(
                                    "[Device Listener] Serialized event: {}.",
                                    as_hex::as_hex(&event_buffer[0..len])
                                );
                                if (*transmitter).try_broadcast((event_buffer, len)).is_err() {
                                    println!("[Device Listener] Bus is full.");
                                }
                            }
                        }
                    }
                }
            }
            Err(error) => {
                println!("[Device Listener] Failed to fetch events: {error:?}.");
            }
        }
    }
}

/// Indicate activity by playing a simple animation on the keyboard LEDs.
/// Wait led_speed_millis between each frame.
fn blink_led(device_name: &String, led_speed_millis: u64) {
    println!("[Blink Led] Searching for device \"{}\".", device_name);
    let mut keyboard = find_device(device_name).expect("unable to find device");

    println!("[Blink Led] Blinking Keyboard LEDs.");
    let duration = Duration::from_millis(led_speed_millis);
    let events = [
        [InputEvent::new(EventType::LED, LedType::LED_NUML.0, 0)],
        [InputEvent::new(EventType::LED, LedType::LED_NUML.0, 1)],
    ];
    loop {
        for event in events {
            keyboard
                .send_events(&event)
                .expect("unable to send LED event");
            std::thread::sleep(duration);
        }
    }
}

/// Handle a TCP connection.
/// After received a null terminated UTF-8 encoded string matching `api_key`,
/// send serialized events (`&event.0[0..event.1]`) from `receiver` until
/// the client disconnects or events can no longer be received from `receiver`.
/// See [`device_listener`] for more details on the event serialization.
fn handle_connection(
    mut stream: std::net::TcpStream,
    api_key: &String,
    mut receiver: BusReader<([u8; 64], usize)>,
) {
    let address = match stream.peer_addr() {
        Ok(addr) => addr.to_string(),
        Err(_) => "UNKNOWN ADDRESS".to_string(),
    };
    println!("[Client {address}] Connection established.");
    let mut buffer_reader = BufReader::new(&mut stream);

    // Receive a null terminated UTF-8 encoded string from the client and validate it against `api_key`.
    let mut client_key = Vec::new();
    match buffer_reader.read_until(0x00, &mut client_key) {
        Err(error) => println!("[Client {address}] Failed to read bytes: {error}."),
        Ok(bytes_read) => {
            println!("[Client {address}] Read {bytes_read} byte API key.");
            if !client_key.starts_with(api_key.as_bytes()) {
                println!("[Client {address}]: Invalid API key.");
                return;
            }
            println!("[Client {address}] Authenticated.");
        }
    }

    // Transmit events received from `receiver` to the client.
    loop {
        match receiver.recv() {
            Ok(event) => {
                if let Err(error) = stream.write(&event.0[0..event.1]) {
                    println!("[Client {address}] Failed to send event: {error}.");
                    return;
                }
            }
            Err(error) => {
                println!("[Client {address}] Failed to receive event from bus: {error}.");
                return;
            }
        }
    }
}

fn main() {
    // List devices.
    list_devices();

    // Load configuration from [this executable's directory]/config.toml].
    let config_file_path = std::env::current_exe()
        .expect("unable to obtain executable directory")
        .parent()
        .expect("unable to obtain executable directory")
        .join("config.toml");
    println!(
        "[Main] Loading configuration file \"{}\".",
        config_file_path.display()
    );
    let config_data = match fs::read_to_string(&config_file_path) {
        Ok(data) => data,
        Err(error) => {
            println!("[Main] Unable to read configuration file: {error}.\nInstalling default.");
            let _ = fs::write(&config_file_path, include_str!("default_config.toml"));
            panic!();
        }
    };

    let config: Config =
        toml::from_str(&config_data).expect("unable to deserialize configuration file");

    // Spawn [`blink_led`].
    let device_name = config.hardware.name.clone();
    let _ = thread::spawn(move || {
        blink_led(&device_name, config.hardware.led_speed_millis);
    });

    // Spawn [`device_listener`].
    let device_name = config.hardware.name.clone();
    let escape_code = config.hardware.escape.code();
    let pause_code = config.hardware.pause.code();
    // `event_bus` is an `Arc<Mutex>` so that it can be mutably borrowed later in [`main`] and in [`device_listener`]
    // because [`main`] adds receivers for each new TCP connection and [`device_listener`] needs to send events.
    let event_bus: Arc<Mutex<Bus<([u8; 64], usize)>>> = Arc::new(Mutex::new(Bus::new(100)));
    let transmitter = Arc::clone(&event_bus);
    let _ = thread::spawn(move || {
        device_listener(&device_name, escape_code, pause_code, transmitter);
    });

    // Accept TCP requests and handle them in `tcp_pool` with [`handle_connection`].
    println!("[Main] Starting TCP server on {}.", config.server.address);
    let tcp_listener =
        std::net::TcpListener::bind(config.server.address).expect("unable to bind TCP listener");
    let tcp_pool = thread_pool::ThreadPool::new(10);
    for stream_result in tcp_listener.incoming() {
        match stream_result {
            Ok(stream) => {
                let api_key = config.server.api_key.clone();
                let receiver = (*event_bus.lock().unwrap()).add_rx(); // This line will block while an input event is processed.
                tcp_pool.execute(move || {
                    handle_connection(stream, &api_key, receiver);
                });
            }
            Err(error) => {
                println!("[Main] Unable to accept connection: {error}");
            }
        }
    }
}
