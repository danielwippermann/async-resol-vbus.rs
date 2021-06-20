# vbus_serial_to_tcp

Provides VBus-over-TCP access using a VBus/USB adapter.


## Synopsis

If you use a serial connection (e.g. VBus/USB adapter or DeltaSol SLT's built-in USB port) to connect to your RESOL controller, you are limited to only one program accessing its serial port at any given time. If you need access with more than one program, the "vbus_serial_to_tcp" example might come in handy.

The "vbus_serial_to_tcp" example provides access to the serial port over a network connection using RESOL's [VBus-over-TCP protocol](http://danielwippermann.github.io/resol-vbus/#/md/docs/vbus-over-tcp). "vbus_serial_to_tcp" is connected to the serial port and forwards data back and forth between the serial port and all connected VBus-over-TCP clients.


## Setup

You need a recent Rust toolchain to compile this example.

```
git clone https://github.com/danielwippermann/async-resol-vbus.rs
cd async-resol-vbus.rs/examples/vbus_serial_to_tcp
cargo build
ls -l target/debug/vbus_serial_to_tcp
```


## Usage

The `vbus_serial_to_tcp` tool accepts several arguments:

- a required `path` argument to specifies the serial port provided by the VBus/USB adapter
- an optional `port` argument that specifies the TCP port to use for the VBus-over-TCP service (default: 7053)


### Example

Starts a VBus-over-TCP service using the VBus/USB adapter accessible under `/dev/tty.usbmodem`
```
target/debug/vbus_serial_to_tcp /dev/tty.usbmodem
```


## Contributors

- [Daniel Wippermann](https://github.com/danielwippermann)


## Legal Notices

RESOL, VBus, VBus.net and others are trademarks or registered trademarks of RESOL - Elektronische Regelungen GmbH.

All other trademarks are the property of their respective owners.


## License

`vbus_serial_to_tcp` is distributed under the terms of both the MIT license and the Apache License (Version 2.0).

See LICENSE.txt for details.
