// This is part of async-resol-vbus.rs.
// Copyright (c) 2020, Daniel Wippermann.
// See README.md and LICENSE.txt for details.

//! # async-resol-vbus.rs
//!
//! A Rust library for processing RESOL VBus data asynchronously.
//!
//!
//! ## Features
//!
//! - Allows discovery of VBus-over-TCP devices in a local network
//! - Connect to or provide VBus-over-TCP services
//!
//!
//! ## Planned, but not yet implemented features
//!
//! - none (yet)
//!
//!
//! ## Examples
//!
//! ```no_run
//! use std::fs::File;
//!
//! use async_std::{
//!     net::{SocketAddr, TcpStream},
//!     prelude::*,
//! };
//!
//! use resol_vbus::{DataSet, RecordingWriter};
//!
//! use async_resol_vbus::{Result, LiveDataStream, TcpClientHandshake};
//!
//! fn main() -> Result<()> {
//!     async_std::task::block_on(async {
//!         // Create an recording file and hand it to a `RecordingWriter`
//!         let file = File::create("test.vbus")?;
//!         let mut rw = RecordingWriter::new(file);
//!     
//!         // Parse the address of the DL2 to connect to
//!         let addr = "192.168.13.45:7053".parse::<SocketAddr>()?;
//!
//!         let stream = TcpStream::connect(addr).await?;
//!
//!         let mut hs = TcpClientHandshake::start(stream).await?;
//!         hs.send_pass_command("vbus").await?;
//!         let stream = hs.send_data_command().await?;
//!
//!         let (reader, writer) = (&stream, &stream);
//!
//!         let mut stream = LiveDataStream::new(reader, writer, 0, 0x0020);
//!
//!         while let Some(data) = stream.receive_any_data(60000).await? {
//!             println!("{}", data.id_string());
//!
//!             // Add `Data` value into `DataSet` to be stored
//!             let mut data_set = DataSet::new();
//!             data_set.timestamp = data.as_ref().timestamp;
//!             data_set.add_data(data);
//!
//!             // Write the `DataSet` into the `RecordingWriter` for permanent storage
//!             rw.write_data_set(&data_set)
//!                 .expect("Unable to write data set");
//!         }
//!
//!         Ok(())
//!     })
//! }
//! ```

#![warn(missing_docs)]
#![deny(missing_debug_implementations)]
#![deny(warnings)]
#![deny(future_incompatible)]
#![deny(nonstandard_style)]
#![deny(rust_2018_compatibility)]
#![deny(rust_2018_idioms)]
#![allow(clippy::if_same_then_else)]
#![allow(clippy::large_enum_variant)]
#![allow(clippy::needless_bool)]
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::trivially_copy_pass_by_ref)]
#![allow(clippy::write_with_newline)]

pub use resol_vbus::*;

mod error;
pub use error::Result;

mod device_information;
pub use device_information::DeviceInformation;

mod device_discovery;
pub use device_discovery::DeviceDiscovery;

mod tcp_client_handshake;
pub use tcp_client_handshake::TcpClientHandshake;

mod tcp_server_handshake;
pub use tcp_server_handshake::TcpServerHandshake;

mod live_data_stream;
pub use live_data_stream::LiveDataStream;
