use std::future::Future;

use async_std::{net::TcpStream, prelude::*};

use resol_vbus::BlobBuffer;

use crate::error::Result;

pub type FutureResult<T> = std::result::Result<T, &'static str>;

/// Handles the server-side of the [VBus-over-TCP][1] handshake.
///
/// [1]: http://danielwippermann.github.io/resol-vbus/vbus-over-tcp.html
///
/// # Examples
///
/// ```no_run
/// # fn main() -> async_resol_vbus::Result<()> { async_std::task::block_on(async {
/// #
/// use async_std::net::{SocketAddr, TcpListener, TcpStream};
///
/// use async_resol_vbus::TcpServerHandshake;
///
/// let address = "0.0.0.0:7053".parse::<SocketAddr>()?;
/// let listener = TcpListener::bind(address).await?;
/// let (stream, _) = listener.accept().await?;
/// let mut hs = TcpServerHandshake::start(stream).await?;
/// let password = hs.receive_pass_command().await?;
/// // ...
/// let stream = hs.receive_data_command().await?;
/// // ...
/// #
/// # Ok(()) }) }
/// ```
#[derive(Debug)]
pub struct TcpServerHandshake {
    stream: TcpStream,
    buf: BlobBuffer,
}

impl TcpServerHandshake {
    /// Start the VBus-over-TCP handshake as the server side.
    pub async fn start(stream: TcpStream) -> Result<TcpServerHandshake> {
        let mut hs = TcpServerHandshake {
            stream,
            buf: BlobBuffer::new(),
        };

        hs.send_reply("+HELLO\r\n").await?;

        Ok(hs)
    }

    /// Consume `self` and return the underlying `TcpStream`.
    pub fn into_inner(self) -> TcpStream {
        self.stream
    }

    async fn send_reply(&mut self, reply: &str) -> Result<()> {
        self.stream.write_all(reply.as_bytes()).await?;
        Ok(())
    }

    async fn receive_line(&mut self) -> Result<String> {
        let line = loop {
            if let Some(idx) = self.buf.iter().position(|b| *b == 10) {
                let string = std::str::from_utf8(&self.buf[0..idx])?.to_string();

                self.buf.consume(idx + 1);

                break string;
            }

            let mut buf = [0u8; 256];
            let len = self.stream.read(&mut buf).await?;
            if len == 0 {
                return Err("Reached EOF".into());
            }

            self.buf.extend_from_slice(&buf[0..len]);
        };

        Ok(line)
    }

    /// Receive a command and verify it and its provided arguments. The
    /// command reception is repeated as long as the verification fails.
    ///
    /// The preferred way to receive commands documented in the VBus-over-TCP
    /// specification is through the `receive_xxx_command` and
    /// `receive_xxx_command_and_verify_yyy` methods which use the
    /// `receive_command` method internally.
    ///
    /// This method takes a validator function that is called with the
    /// received command and its optional arguments. The validator
    /// returns a `Future` that can resolve into an
    /// `std::result::Result<T, &'static str>`. It can either be:
    /// - `Ok(value)` if the validation succeeded. The `value` is used
    ///   to resolve the `receive_command` `Future`.
    /// - `Err(reply)` if the validation failed. The `reply` is send
    ///   back to the client and the command reception is repeated.
    pub async fn receive_command<V, R, T>(&mut self, validator: V) -> Result<T>
    where
        V: Fn(String, Option<String>) -> R,
        R: Future<Output = FutureResult<T>>,
    {
        let result = loop {
            let line = self.receive_line().await?;
            let line = line.trim();

            let (command, args) = if let Some(idx) = line.chars().position(|c| c.is_whitespace()) {
                let command = (&line[0..idx]).to_uppercase();
                let args = (&line[idx..]).trim().to_string();
                (command, Some(args))
            } else {
                (line.to_uppercase(), None)
            };

            let (reply, result) = if command == "QUIT" {
                ("+OK\r\n", Some(Err("Received QUIT command".into())))
            } else {
                match validator(command, args).await {
                    Ok(result) => ("+OK\r\n", Some(Ok(result))),
                    Err(reply) => (reply, None),
                }
            };

            self.send_reply(reply).await?;

            if let Some(result) = result {
                break result;
            }
        };

        result
    }

    /// Wait for a `CONNECT <via_tag>` command. The via tag argument is returned.
    pub async fn receive_connect_command(&mut self) -> Result<String> {
        self.receive_connect_command_and_verify_via_tag(|via_tag| async move { Ok(via_tag) })
            .await
    }

    /// Wait for a `CONNECT <via_tag>` command.
    pub async fn receive_connect_command_and_verify_via_tag<V, R>(
        &mut self,
        validator: V,
    ) -> Result<String>
    where
        V: Fn(String) -> R,
        R: Future<Output = FutureResult<String>>,
    {
        self.receive_command(|command, args| {
            let result = if command != "CONNECT" {
                Err("-ERROR Expected CONNECT command\r\n")
            } else if let Some(via_tag) = args {
                Ok(validator(via_tag))
            } else {
                Err("-ERROR Expected argument\r\n")
            };

            async move {
                match result {
                    Ok(future) => future.await,
                    Err(err) => Err(err),
                }
            }
        })
        .await
    }

    /// Wait for a `PASS <password>` command.
    pub async fn receive_pass_command(&mut self) -> Result<String> {
        self.receive_pass_command_and_verify_password(|password| async move { Ok(password) })
            .await
    }

    /// Wait for a `PASS <password>` command and validate the provided password.
    pub async fn receive_pass_command_and_verify_password<V, R>(
        &mut self,
        validator: V,
    ) -> Result<String>
    where
        V: Fn(String) -> R,
        R: Future<Output = FutureResult<String>>,
    {
        self.receive_command(|command, args| {
            let result = if command != "PASS" {
                Err("-ERROR Expected PASS command\r\n")
            } else if let Some(password) = args {
                Ok(validator(password))
            } else {
                Err("-ERROR Expected argument\r\n")
            };

            async move {
                match result {
                    Ok(future) => future.await,
                    Err(err) => Err(err),
                }
            }
        })
        .await
    }

    /// Wait for a `CHANNEL <channel>` command.
    pub async fn receive_channel_command(&mut self) -> Result<u8> {
        self.receive_channel_command_and_verify_channel(|channel| async move { Ok(channel) })
            .await
    }

    /// Wait for `CHANNEL <channel>` command and validate the provided channel
    pub async fn receive_channel_command_and_verify_channel<V, R>(
        &mut self,
        validator: V,
    ) -> Result<u8>
    where
        V: Fn(u8) -> R,
        R: Future<Output = FutureResult<u8>>,
    {
        self.receive_command(|command, args| {
            let result = if command != "CHANNEL" {
                Err("-ERROR Expected CHANNEL command\r\n")
            } else if let Some(channel) = args {
                if let Ok(channel) = channel.parse() {
                    Ok(validator(channel))
                } else {
                    Err("-ERROR Expected 8 bit number argument\r\n")
                }
            } else {
                Err("-ERROR Expected argument\r\n")
            };

            async {
                match result {
                    Ok(future) => future.await,
                    Err(err) => Err(err),
                }
            }
        })
        .await
    }

    /// Wait for a `DATA` command.
    ///
    /// This function returns the underlying `TcpStream` since the handshake is complete
    /// after sending this command.
    pub async fn receive_data_command(mut self) -> Result<TcpStream> {
        self.receive_command(|command, args| {
            let result = if command != "DATA" {
                Err("-ERROR Expected DATA command\r\n")
            } else if args.is_some() {
                Err("-ERROR Unexpected argument\r\n")
            } else {
                Ok(())
            };

            async move { result }
        })
        .await?;

        Ok(self.stream)
    }
}
