use std::{
    time::Duration,
};

use async_resol_vbus::{
    Result,
    LiveDataBuffer,
    TcpServerHandshake,
};

use async_std::{
    channel::{Receiver, Sender},
    net::{SocketAddr, TcpListener, TcpStream},
    prelude::*,
    sync::{Arc, Mutex},
};

use clap::{App, Arg};

use log::{error, trace};

use serialport::SerialPort;

fn wrap_err<T, E: std::fmt::Debug>(message: &str, err: E) -> Result<T> {
    Err(format!("{}: {:?}", message, err).into())
}

fn run_serial_read_loop(mut rx_port: Box<dyn SerialPort>, rx_port_sender: Sender<Vec<u8>>) -> Result<()> {
    let mut ldb = LiveDataBuffer::new(0);
    let mut buf = [0; 4096];
    loop {
        // trace!("Start reading from serial port...");
        let size = match rx_port.read(&mut buf) {
            Ok(size) => size,
            Err(err) if err.kind() == std::io::ErrorKind::TimedOut => continue,
            Err(err) => return wrap_err("Unable to read from serial port", err),
        };

        ldb.extend_from_slice(&buf [0..size]);
        while let Some(data) = ldb.read_data() {
            trace!("Received {}", data.id_string());
        }

        // trace!("Read {} bytes from serial port...", size);
        let mut v = Vec::with_capacity(size);
        v.extend_from_slice(&buf [0..size]);

        // trace!("Sending {} bytes to rx port channel...", size);
        match rx_port_sender.try_send(v) {
            Ok(_) => {},
            Err(err) => return wrap_err("Unable to send to rx port channel", err),
        }
    }
}

async fn run_serial_write_loop(mut tx_port: Box<dyn SerialPort>, tx_port_receiver: Receiver<Vec<u8>>) -> Result<()> {
    loop {
        // trace!("Start receiving from tx port channel...");
        let buf = match tx_port_receiver.recv().await {
            Ok(buf) => buf,
            Err(err) => return wrap_err("Unable to receive from TX port channel", err),
        };

        // trace!("Write {} bytes to serial port...", buf.len());
        tx_port.write_all(&buf)?;
    }
}

fn remove_stream_with_id(tx_clients: &mut Vec<(usize, TcpStream)>, stream_id: usize) {
    trace!("Searching for stream with ID {}...", stream_id);
    if let Some(pos) = tx_clients.iter().position(|(sid, _)| *sid == stream_id) {
        trace!("   ... found it at pos {}, removing it now...", pos);
        tx_clients.remove(pos);
    }
}

async fn run_client_loop(stream_id: usize, stream: TcpStream, tx_clients: Arc<Mutex<Vec<(usize, TcpStream)>>>, tx_port_sender: Sender<Vec<u8>>) -> Result<()> {
    trace!("Starting VBus-over-TCP handshake on stream ID {} from {:?}...", stream_id, stream.peer_addr());
    let mut hs = TcpServerHandshake::start(stream).await?;
    let _password = hs.receive_pass_command().await?;

    // TODO(daniel): optionally compare password here

    let mut stream = hs.receive_data_command().await?;

    trace!("VBus-over-TCP handshake complete for stream ID {}...", stream_id);
    {
        let mut tx_clients = tx_clients.lock().await;

        tx_clients.push((stream_id, stream.clone()));
    }

    let mut buf = [0; 4096];

    let result = loop {
        // trace!("Start reading from client stream {}...", stream_id);
        let size = match stream.read(&mut buf).await {
            Ok(0) => break Ok(()),
            Ok(size) => size,
            Err(err) => {
                return Err(format!("Unable to read from client: {:?}", err).into());
            },
        };

        // trace!("Read {} bytes from client stream {}...", size, stream_id);
        let mut v = Vec::with_capacity(size);
        v.extend_from_slice(&buf [0..size]);

        // trace!("Sending {} bytes from client stream {} to tx port channel...", size, stream_id);
        match tx_port_sender.send(v).await {
            Ok(_) => {},
            Err(err) => break wrap_err("Unable to send to tx port channel", err),
        }
    };

    // trace!("Shutting down stream ID {}...", stream_id);
    drop(stream.shutdown(std::net::Shutdown::Both));

    {
        let mut tx_clients = tx_clients.lock().await;

        remove_stream_with_id(&mut tx_clients, stream_id);
    }

    result
}

async fn run_clients_write_loop(tx_clients: Arc<Mutex<Vec<(usize, TcpStream)>>>, rx_port_receiver: Receiver<Vec<u8>>) -> Result<()> {
    loop {
        // trace!("Start receiving from rx port channel...");
        let buf = match rx_port_receiver.recv().await {
            Ok(buf) => buf,
            Err(err) => return wrap_err("Unable to receive from rx port channel", err),
        };

        // trace!("Received {} bytes from rx port channel...", buf.len());
        {
            let mut tx_clients = tx_clients.lock().await;

            let mut err_stream_ids = Vec::new();
            for (stream_id, tx_client) in tx_clients.iter_mut() {
                match tx_client.write_all(&buf).await {
                    Ok(_) => {},
                    Err(err) => {
                        error!("Error while writing to stream ID {}: {:?}", *stream_id, err);
                        err_stream_ids.push(*stream_id);
                    },
                }
            }

            for stream_id in err_stream_ids {
                remove_stream_with_id(&mut tx_clients, stream_id);
            }
        }
    }
}

async fn run_main_loop() -> Result<()> {
    let matches = App::new("vbus_serial_to_tcp")
        .arg(Arg::with_name("path")
            .index(1)
            .required(true)
            .takes_value(true))
        .arg(Arg::with_name("port")
            .index(2)
            .required(false)
            .takes_value(true))
        .get_matches();

    let path = matches.value_of("path").expect("No path provided");

    let port = match matches.value_of("port") {
        Some(port) => {
            match port.parse::<u16>() {
                Ok(port) => port,
                Err(err) => return wrap_err("Unable to parse port", err),
            }
        },
        None => 7053,
    };

    let address = format!("0.0.0.0:{}", port);
    let address = address.parse::<SocketAddr>()?;
    let listener = TcpListener::bind(address).await?;
    let mut incoming = listener.incoming();

    let tx_clients = Arc::new(Mutex::new(Vec::new()));

    let tx_port = match serialport::new(path, 9600).timeout(Duration::from_secs(10)).open() {
        Ok(serialport) => serialport,
        Err(err) => return wrap_err("Unable to open serial port", err),
    };

    let rx_port = match tx_port.try_clone() {
        Ok(serialport) => serialport,
        Err(err) => return wrap_err("Unable to clone serial port", err),
    };

    let (tx_port_sender, tx_port_receiver) = async_std::channel::bounded(10);
    let (rx_port_sender, rx_port_receiver) = async_std::channel::bounded(10);

    std::thread::spawn(move || {
        let result = run_serial_read_loop(rx_port, rx_port_sender);
        panic!("Serial read loop should not have ended: {:?}", result);
    });

    async_std::task::spawn(async move {
        let result = run_serial_write_loop(tx_port, tx_port_receiver).await;
        panic!("Serial write loop should not have ended: {:?}", result);
    });

    {
        let tx_clients = tx_clients.clone();

        async_std::task::spawn(async move {
            let result = run_clients_write_loop(tx_clients, rx_port_receiver).await;
            panic!("Serial write loop should not have ended: {:?}", result);
        });
    }

    let mut next_stream_id = 0;

    while let Some(stream) = incoming.next().await {
        let stream = stream?;

        let stream_id = next_stream_id;
        next_stream_id += 1;

        let tx_clients = tx_clients.clone();
        let tx_port_sender = tx_port_sender.clone();

        trace!("Spawning task for stream ID {}...", stream_id);
        async_std::task::spawn(async move {
            trace!("Starting client loop for stream ID {}...", stream_id);
            match run_client_loop(stream_id, stream, tx_clients, tx_port_sender).await {
                Ok(_) => {},
                Err(err) => {
                    error!("Client loop for stream ID {} ended with error: {:?}", stream_id, err);
                },
            }
        });
    }

    Ok(())
}

fn main() -> Result<()> {
    env_logger::init();

    async_std::task::block_on(async {
        match run_main_loop().await {
            Ok(_) => {},
            Err(err) => {
                error!("The main loop ended: {:?}", err);
            }
        }

        Ok(())
    })
}
