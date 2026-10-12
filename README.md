# PerifeMngr

A native Windows app for managing PC peripherals in one place: mice, keyboards, headsets, microphones and
speakers, plus a full flight sim setup with the **Logitech X56 H.O.T.A.S.** stick and throttle and the
**Saitek Pro Flight Rudder Pedals**. Written in Rust with [egui](https://github.com/emilk/egui).
Formerly called Joystick Manager.

## Features

- **Peripherals overview** – a card with the product photo and key facts for every supported peripheral:
  Logitech G309 mouse, Razer Kraken V4 X headset, HyperX QuadCast S microphone, Vieta Pro Astra speaker and
  Newskill Serike V2 keyboard, next to the HOTAS.
- **Audio** – volume, mute, a live level meter and the default device for every speaker, headset and
  microphone, grouped by hardware.
- **Logitech mouse** – battery, DPI and report rate of Logitech mice over the HID++ protocol, without
  Logitech's software.
- **Overview** – connection state, VID:PID, serial number, driver version and current colour for every device.
- **Drivers** – every USB interface with its driver provider, version, date and INF, compared against the
  latest Logitech release. Searches Windows Update for newer signed drivers and installs them in one click.
- **Lighting** – colour and brightness for the X56 stick and throttle through Logitech's DirectOutput SDK,
  with presets, effects (breathing, rainbow, two-colour fade, strobe) at any speed, a "link both units" switch and changes shown on the hardware live.
- **Input tester** – live axes, buttons and hats at 100 Hz, axis jitter measurement, range recording and a
  button press log to catch ghost inputs.
- **Key mapping** – bind any button, hat or axis to a keyboard key, mouse click, wheel or pointer movement, or to
  a virtual Xbox or PlayStation (DualShock 4) controller. Mapping reads the devices' HID reports directly and
  reacts to each report the moment it arrives, so it keeps working while a game has focus. The virtual
  controller needs the free [ViGEmBus](https://github.com/nefarius/ViGEmBus/releases/latest) driver.
- **Stick drift correction** – measures every stick axis at rest for 30 seconds to find where it really rests
  and how much it wanders, then re-centres it and hides the jitter for everything the app sends. The input
  tester shows corrected values, with a switch to see the raw readings.
- **Modes and layers** – the X56 throttle's M1 / M2 / M3 switch is read straight from the throttle, and each
  position can load its own profile the moment it is flipped. An optional shift button gives every control a
  second job.
- **Hall effect keys** – an axis drives a key the way a magnetic (Hall effect) keyboard switch does: an
  adjustable actuation point, rapid trigger that releases the key the moment the axis eases off, and a
  continuous rapid trigger mode. For games that only read keys there is also a pulsed key, held for a share of
  the time that follows the axis. Real partial input goes through the virtual controller's sticks and triggers.
- **Press styles** – a button can act while held, toggle on and off, or fire only on a short tap, a long hold
  or a double tap, so one button can do several things.
- **Profiles** – named setups with lighting, axis tuning and key mapping, activated manually or automatically
  when a game starts. Import and export as JSON.
- **USB power saving** – turns off the Windows power saving that is a common cause of X56 disconnects.
- **Tray and start-up** – closes to the notification area, starts with Windows (elevated, via Task Scheduler),
  dims the lights when the PC is idle.
- **Automatic updates** – new GitHub releases are downloaded, checked against their published SHA-256 checksum
  and installed silently.
- **English and Spanish** – the language follows Windows and can be changed in Settings.

See [ROADMAP.md](ROADMAP.md) for what is planned next.

## Install

Download `PerifeMngr-Setup-x.y.z.exe` from the
[latest release](https://github.com/NNKTV28/JoistickMngr/releases/latest) and run it. The installer adds a
Start menu entry (so the app shows up in Windows search) and can register the app to start when you sign in.

Lighting needs the Logitech X56 software installed, because it provides the DirectOutput service.

## Build from source

```powershell
cargo build --release
```

Debug builds run without administrator rights; release builds ask for elevation because driver installs,
device restarts and power settings need it.

To build the installer locally, install [Inno Setup 6](https://jrsoftware.org/isinfo.php) and run:

```powershell
./scripts/prepare-signing-certificate.ps1
./scripts/sign-file.ps1 -FilePath target/release/PerifeMngr.exe
./scripts/build-installer.ps1 -Version 0.1.0
```

## Releases and signing

Pushing a `v*` tag runs `.github/workflows/release.yml`, which builds, signs and publishes the installer.
The signing certificate comes from the `SIGNING_CERTIFICATE_BASE64` and `SIGNING_CERTIFICATE_PASSWORD`
repository secrets; `scripts/new-signing-certificate.ps1` creates a NikiCoding certificate and stores it there.

## License

MIT © [NikiCoding](https://nikicoding.com)
