# vbus_serial_to_tcp

Provides VBus-over-TCP access using a VBus/USB adapter.


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
