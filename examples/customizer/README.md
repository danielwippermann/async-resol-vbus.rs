# customizer

Allows to read or write controller parameters using a VBus-over-TCP service.


## Features

- Connects to a VBus-over-TCP service (e.g. for a datalogger or VBus.net)
- Read and write controller parameters
- Optionally looks up values by name
- Optionally use parameter file to make name lookup faster and apply min/max constraints


## Setup

You need a recent Rust toolchain to compile this example.

```
git clone https://github.com/danielwippermann/async-resol-vbus.rs
cd async-resol-vbus.rs/examples/customizer
cargo build
ls -l target/debug/customizer
```


## Usage

```
customizer 0.1
Daniel Wippermann <daniel.wippermann@gmail.com>
Communicate with RESOL VBus device to get or set parameters

USAGE:
    customizer [OPTIONS] <actions>...

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
        --channel <CHANNEL>      Set the channel to communicate over
        --host <HOST>            Set the host to communicate with
        --paramFile <FILE>       Set a parameter TOML filename
        --password <PASSWORD>    Set the password for the RESOL VBus device
        --port <PORT>            Set the port to communicate with
        --viaTag <VIATAG>        Set the via tag to communicate with

ARGS:
    <actions>...
```

The `customizer` tool accepts several arguments:

- a `--host <HOST>` argument that specifies the VBus-over-TCP service to connect to
- an optional `--port <PORT>` argument that specifies the VBus-over-TCP port to connect to (defaults to 7053)
- an optional `--viaTag <VIATAG>` argument that specifies which VBus.net device to connect to
- a `--password <PASSWORD>` argument that specifies the password to offer during the VBus-over-TCP handshake
- a `--channel <CHANNEL>` argument that specifies the VBus channel to connect to (only for multi-channel devices like the DL3)
- an optional `--paramFile <FILE>` argument that specifies a file describing the parameters of the device to be customized
- a list of actions in the form of `<VALUE_ID_OR_INDEX>=<VALUE>` where `<VALUE_ID_OR_INDEX>` can either be a value ID like `Relais_Regler_R1_Handbetrieb` or a value index like `0x0916` and where `<VALUE>` can either be a number if you want to write the value or a question mark if you want to read the value


### Parameter files

A parameter file is a text file according to the TOML specification. It includes information like the controller's address and optionally the changeset this file applies to. In addition to that it contains the list of well-known parameters, consisting of the following information:

- ID of the parameter
- optionally the index of the parameter (tied to this specific changeset)
- the factor used to convert the stored value
- the allowed minimum and maximum value


**TODO(daniel)** include some parameter files in the example


### Example

Since the names of parameters differ between controller families, this example assumes that you are connected to a DeltaSol MX.

Read the value of `Relais_Regler_R1_Handbetrieb`:

```
target/debug/customizer --host 127.0.0.1 --password vbus 'Relais_Regler_R1_Handbetrieb=?'
```

Write the value of `Relais_Regler_R1_Handbetrieb` to set it to automatic (=2):

```
target/debug/customizer --host 127.0.0.1 --password vbus 'Relais_Regler_R1_Handbetrieb=2'
```


## Contributors

- [Daniel Wippermann](https://github.com/danielwippermann)


## Legal Notices

RESOL, VBus, VBus.net and others are trademarks or registered trademarks of RESOL - Elektronische Regelungen GmbH.

All other trademarks are the property of their respective owners.


## License

`customizer` is distributed under the terms of both the MIT license and the Apache License (Version 2.0).

See LICENSE.txt for details.
