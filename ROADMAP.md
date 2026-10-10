# Roadmap

## Done

- Device overview, driver status, Windows Update driver search and repair of blocked drivers
- RGB lighting for the X56 stick and throttle
- Live input tester with axis jitter and a ghost input log
- Profiles with automatic switching when a game starts
- Key, mouse and virtual Xbox / DualShock 4 controller mapping
- Mode layers (M1 / M2 / M3 switch) and a shift layer
- Analog keys: a key pulsed in proportion to how far an axis is pushed
- Axis deadzone, curve and saturation applied to mapped axes
- Automatic updates from GitHub releases
- English and Spanish interface

## Planned

Next in line, roughly in this order.

- **Tap, hold and double-tap** – different actions depending on how a button is pressed, plus a toggle mode
  that keeps a key held until the next press.
- **Macros** – one button sends a sequence of keys with delays.
- **Hide the real HOTAS from games** – use HidHide so games only see the virtual controller and never get
  input twice.
- **Throttle zones** – fire keys when a throttle crosses set points, such as an afterburner detent.

## To do

Ideas that are wanted but not scheduled yet.

- **Combine axes** – treat both toe brakes or both throttles as one axis.
- **Controller order** – keep the order Windows reports controllers in stable for games that bind by position.
- **Reactive lighting** – colour per mode, flash on button presses, colour that follows the throttle.
- **Game-driven lighting** – LEDs react to the simulator (DCS, MSFS) through their export interfaces.
- **Cheat sheet** – printable sheet or on-screen overlay of what every button does in the current profile.
- **Disconnect watchdog** – notify when a device drops out and restore its lighting when it returns.
- **Calibration** – set each axis's minimum, maximum and centre to compensate for wear.
- **Custom drivers** – explore own drivers to improve how the devices behave in Windows.
