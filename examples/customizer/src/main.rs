use std::{
    net::SocketAddr,
};

use async_resol_vbus::{
    LiveDataStream,
    Result,
    TcpClientHandshake,
};

use async_std::net::TcpStream;

use clap::{Arg, App};

use log::{trace, warn};

use serde::{Deserialize};

fn wrap_err<T, E: std::fmt::Debug>(message: &str, err: E) -> Result<T> {
    Err(format!("{}: {:?}", message, err).into())
}

#[derive(Clone, Debug, Default, Deserialize)]
struct Parameter {
    id: Option<String>,
    index: Option<i16>,
    factor: f64,
    minimum: f64,
    maximum: f64,
}

#[derive(Debug, Default, Deserialize)]
struct ParameterFile {
    address: u16,
    changeset: u32,
    params: Vec<Parameter>,
}

struct Transaction {
    id_or_index: String,
    index: i16,
    param: Parameter,
    value: Option<f64>,
}

fn value_id_hash_by_id(id: &str) -> i32 {
    id.chars().fold(0, |acc, c| {
        acc.wrapping_mul(0x21).wrapping_add(c as i32) & 0x7fffffff
    })
}

fn main() -> Result<()> {
    env_logger::init();

    async_std::task::block_on(async {
        let matches = App::new("customizer")
            .version("0.1")
            .author("Daniel Wippermann <daniel.wippermann@gmail.com>")
            .about("Communicate with RESOL VBus device to get or set parameters")
            .arg(Arg::with_name("param_filename")
                .long("paramFile")
                .value_name("FILE")
                .help("Set a parameter TOML filename")
                .takes_value(true))
            .arg(Arg::with_name("host")
                .long("host")
                .value_name("HOST")
                .help("Set the host to communicate with")
                .takes_value(true))
            .arg(Arg::with_name("port")
                .long("port")
                .value_name("PORT")
                .help("Set the port to communicate with")
                .takes_value(true))
            .arg(Arg::with_name("via_tag")
                .long("viaTag")
                .value_name("VIATAG")
                .help("Set the via tag to communicate with")
                .takes_value(true))
            .arg(Arg::with_name("password")
                .long("password")
                .value_name("PASSWORD")
                .help("Set the password for the RESOL VBus device")
                .takes_value(true))
            .arg(Arg::with_name("channel")
                .long("channel")
                .value_name("CHANNEL")
                .help("Set the channel to communicate over")
                .takes_value(true))
            .arg(Arg::with_name("actions")
                .required(true)
                .index(1)
                .multiple(true))
            .get_matches();

        let param_file = match matches.value_of("param_filename") {
            Some(param_filename) => {
                let param_file_string = std::fs::read_to_string(param_filename)?;
                let param_file = match toml::from_str::<ParameterFile>(&param_file_string) {
                    Ok(param_file) => param_file,
                    Err(err) => return wrap_err("Unable to parse parameters TOML file", err),
                };
                Some(param_file)
            },
            None => None,
        };

        let host = if let Some(host) = matches.value_of("host") {
            host
        } else {
            return Err("No host param provided".into());
        };

        let port = if let Some(port) = matches.value_of("port") {
            match port.parse::<u16>() {
                Ok(port) => port,
                Err(err) => return wrap_err("Unable to parse port number", err),
            }
        } else {
            7053
        };

        let channel = if let Some(channel) = matches.value_of("channel") {
            match channel.parse::<u8>() {
                Ok(channel) => Some(channel),
                Err(err) => return wrap_err("Unable to parse channel", err),
            }
        } else {
            None
        };

        let actions = if let Some(actions) = matches.values_of("actions") {
            actions
        } else {
            return Err("No actions provided".into());
        };

        let address = format!("{}:{}", host, port);
        let address = address.parse::<SocketAddr>()?;

        trace!("Connecting to {:?}", address);
        let stream = TcpStream::connect(address).await?;

        trace!("Starting VBus-over-TCP handshake...");
        let mut hs = TcpClientHandshake::start(stream).await?;

        if let Some(via_tag) = matches.value_of("via_tag") {
            trace!("Sending CONNECT command for {:?}", via_tag);
            hs.send_connect_command(via_tag).await?;
        }

        if let Some(password) = matches.value_of("password") {
            trace!("Sending PASS command");
            hs.send_pass_command(password).await?;
        }

        if let Some(channel) = channel {
            trace!("Sending CHANNEL command for {:?}", channel);
            hs.send_channel_command(channel).await?;
        }

        trace!("Sending DATA command");
        let stream = hs.send_data_command().await?;

        let mut stream = LiveDataStream::new(stream.clone(), stream, 0, 0x0020);

        trace!("Waiting for free bus...");
        let peer_address = match stream.wait_for_free_bus().await? {
            Some(dgram) => dgram.header.source_address,
            None => return Err("Unable to get free bus".into()),
        };

        trace!("Peer address is 0x{:04X}", peer_address);

        trace!("Reading changeset ID...");
        let changeset = match stream.get_value_by_index(peer_address, 0, 0).await? {
            Some(dgram) => dgram.param32 as u32,
            None => 0,
        };

        trace!("Changeset ID is 0x{:08X}", changeset);

        if let Some(param_file) = &param_file {
            if peer_address != param_file.address {
                return Err(format!("Expected address to be 0x{:04X}, but got 0x{:04X}", param_file.address, peer_address).into());
            }
            if changeset != param_file.changeset {
                return Err(format!("Expected changeset to be 0x{:08X}, but got 0x{:08X}", param_file.changeset, changeset).into());
            }
        }

        let mut transactions = Vec::new();
        let mut needs_resync = false;
        for action in actions {
            let mut iter = action.splitn(2, '=');
            if let (Some(id_or_index), Some(value)) = (iter.next(), iter.next()) {
                let starts_with_number = id_or_index.starts_with(|c: char| c.is_numeric());
                trace!("Action {:?} starts_with_number = {:?}", action, starts_with_number);

                let index = if starts_with_number {
                    trace!("Parsing index {:?}...", id_or_index);
                    let result = if id_or_index.starts_with("0x") {
                        i16::from_str_radix(&id_or_index [2..], 16)
                    } else {
                        i16::from_str_radix(id_or_index, 10)
                    };

                    match result {
                        Ok(index) => Some(index),
                        Err(err) => return wrap_err("Unable to parse action index", err),
                    }
                } else {
                    None
                };
                trace!("Index = {:?}", index);

                let mut param = if let Some(param_file) = &param_file {
                    let param = if let Some(index) = index {
                        param_file.params.iter().find(|p| p.index == Some(index))
                    } else {
                        param_file.params.iter().find(|p| p.id.as_ref().map(|s| s.as_str()) == Some(id_or_index))
                    };

                    if let Some(param) = param {
                        param.clone()
                    } else {
                        return Err(format!("Unable to find parameter for action {:?}", action).into());
                    }
                } else if let Some(index) = index {
                    Parameter {
                        id: None,
                        index: Some(index),
                        factor: 1.0,
                        minimum: f64::from(i32::MIN),
                        maximum: f64::from(i32::MAX),
                    }
                } else {
                    Parameter {
                        id: Some(id_or_index.to_string()),
                        index: None,
                        factor: 1.0,
                        minimum: f64::from(i32::MIN),
                        maximum: f64::from(i32::MAX),
                    }
                };

                let index = if let Some(index) = &param.index {
                    *index
                } else if let Some(id) = &param.id {
                    let id_hash = value_id_hash_by_id(id);
                    trace!("value_id_hash_by_id({:?}) = 0x{:08X}", id, id_hash);

                    let index = match stream.get_value_index_by_id_hash(peer_address, id_hash).await? {
                        Some(dgram) => {
                            if dgram.command == 0x0100 {
                                needs_resync = true;
                            }
                            dgram.param16
                        },
                        None => {
                            0
                        },
                    };

                    if index == 0 {
                        return Err(format!("Unable to get index for param {:?}", param.id).into());
                    }

                    trace!("value_index_by_id_hash(0x{:08X}) = 0x{:04X}", id_hash, index);
                    param.index = Some(index);

                    index
                } else {
                    return Err(format!("Unable to determine index for action {:?}", action).into());
                };

                let value = if value == "?" {
                    None
                } else if let Ok(value) = value.parse::<f64>() {
                    Some(value)
                } else {
                    return Err(format!("Unable to parse value in action {:?}", action).into());
                };

                transactions.push(Transaction {
                    id_or_index: id_or_index.to_string(),
                    index,
                    param,
                    value,
                });
            } else {
                return Err(format!("Unable to parse action {:?}", action).into());
            }
        }

        if needs_resync {
            trace!("Resyncing bus");
            stream.get_value_by_index(peer_address, 0, 0).await?;
        }

        for transaction in transactions.iter_mut() {
            let index = transaction.index;

            let rx_value = if let Some(tx_value) = transaction.value {
                let tx_value = if tx_value < transaction.param.minimum {
                    let min = transaction.param.minimum;
                    warn!("Limiting value for {:?} to min of {}", transaction.id_or_index, min);
                    min
                } else if tx_value > transaction.param.maximum {
                    let max = transaction.param.maximum;
                    warn!("Limiting value for {:?} to max of {}", transaction.id_or_index, max);
                    max
                } else {
                    tx_value
                };

                let tx_value = (tx_value / transaction.param.factor).round() as i32;

                trace!("set_value_by_index() with index = 0x{:04X} and value = {}", index, tx_value);
                match stream.set_value_by_index(peer_address, index, 0, tx_value).await? {
                    Some(dgram) if dgram.command == 0x0100 => {
                        Some(dgram.param32)
                    },
                    _ => None,
                }
            } else {
                trace!("get_value_by_index() with index = 0x{:04X}", index);
                match stream.get_value_by_index(peer_address, index, 0).await? {
                    Some(dgram) if dgram.command == 0x0100  => {
                        Some(dgram.param32)
                    },
                    _ => None,
                }
            };

            transaction.value = if let Some(value) = rx_value {
                Some(f64::from(value) * transaction.param.factor)
            } else {
                None
            };
        }

        trace!("Releasing bus");
        drop(stream.release_bus(peer_address).await);

        for transaction in &transactions {
            let value = match transaction.value {
                Some(value) => format!("{}", value),
                None => "?".to_string(),
            };

            println!("{}={}", transaction.id_or_index, value);
        }

        Ok(())
    })
}
