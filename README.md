# Remote Input Server

This is a rust server that sends events from an attached keyboard over the network to compatible clients such as [bwestley/soundboard](https://github.com/bwestley/soundboard).

## Features

* Simple network protocol
* Basic API key authentication (UNSECURE OVER A CLEAR CHANNEL)
* TOML configuration
* Grab and ungrab device, blocking keyboard events from the rest of the system
* Pause and unpause event transmission to all clients

## Configuration

The configuration is loaded from the "config.toml" file in the executable's directory. If it is unreadable, the default configuration is installed.

Default configuration:
```toml
[hardware]
# The name of the keyboard device as reported by evdev:
name = "Logitech USB Keyboard"
# The status light blink duration in milliseconds
led_speed_millis = 3000
# See https://github.com/torvalds/linux/blob/master/include/uapi/linux/
# input-event-codes.h for key names.
# The escape key will ungrab and grab the input device.
escape = "KEY_SCROLLLOCK"
# The pause key will pause and unpause event transmission.
pause = "KEY_PAUSE"

[server]
# The bind address for the remote input server:
address = "0.0.0.0:8650"
# The api key (terminated by a zero byte) must be sent by
# the client when the connection is established.
api_key = "d4AXBDqWa0PQgsGVc4oKnguYA4jEfu5EM7ztD7to"
```

## Network Protocol

Events are converted into the `InputEventWrapper` struct before being serialized by [`postcard`](https://github.com/jamesmunns/postcard) and encoded by [COBS](https://en.wikipedia.org/wiki/Consistent_Overhead_Byte_Stuffing). The event types and codes can be found in <https://github.com/torvalds/linux/blob/master/include/uapi/linux/input-event-codes.h>. For an example decoding this data, see <https://github.com/bwestley/soundboard/blob/master/src/input.rs> and <https://github.com/bwestley/soundboard/blob/master/src/event.rs>.
```rust
struct InputEventWrapper {
    timestamp: std::time::SystemTime,
    event_type: u16,
    code: u16,
    value: i32,
}
struct std::time::SystemTime {
    tv_sec: i64,
    tv_nsec: u32,
}
```
