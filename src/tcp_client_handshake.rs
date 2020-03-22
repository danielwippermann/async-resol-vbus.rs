use async_std::{net::TcpStream, prelude::*};

use resol_vbus::BlobBuffer;

use crate::error::Result;

/// Handles the client-side of the [VBus-over-TCP][1] handshake.
///
/// [1]: http://danielwippermann.github.io/resol-vbus/vbus-over-tcp.html
///
/// # Examples
///
/// ```no_run
/// # fn main() -> async_resol_vbus::Result<()> { async_std::task::block_on(async {
/// #
/// use async_std::net::{SocketAddr, TcpStream};
///
/// use async_resol_vbus::TcpClientHandshake;
///
/// let address = "192.168.5.217:7053".parse::<SocketAddr>()?;
/// let stream = TcpStream::connect(address).await?;
/// let mut hs = TcpClientHandshake::start(stream).await?;
/// hs.send_pass_command("vbus").await?;
/// let stream = hs.send_data_command().await?;
/// // ...
/// #
/// # Ok(()) }) }
/// ```
#[derive(Debug)]
pub struct TcpClientHandshake {
    stream: TcpStream,
    buf: BlobBuffer,
}

impl TcpClientHandshake {
    /// Start the handshake by waiting for the initial greeting reply from the service.
    pub async fn start(stream: TcpStream) -> Result<TcpClientHandshake> {
        let mut hs = TcpClientHandshake {
            stream,
            buf: BlobBuffer::new(),
        };

        hs.read_reply().await?;

        Ok(hs)
    }

    /// Consume `self` and return the underlying `TcpStream`.
    pub fn into_inner(self) -> TcpStream {
        self.stream
    }

    async fn read_reply(&mut self) -> Result<()> {
        let first_byte = loop {
            if let Some(idx) = self.buf.iter().position(|b| *b == 10) {
                let first_byte = self.buf[0];
                self.buf.consume(idx + 1);

                break first_byte;
            }

            let mut buf = [0u8; 256];
            let len = self.stream.read(&mut buf).await?;
            if len == 0 {
                return Err("Reached EOF".into());
            }

            self.buf.extend_from_slice(&buf[0..len]);
        };

        if first_byte == b'+' {
            Ok(())
        } else if first_byte == b'-' {
            Err("Negative reply".into())
        } else {
            Err("Unexpected reply".into())
        }
    }

    async fn send_command(&mut self, cmd: &str, args: Option<&str>) -> Result<()> {
        let cmd = match args {
            Some(args) => format!("{} {}\r\n", cmd, args),
            None => format!("{}\r\n", cmd),
        };

        self.stream.write_all(cmd.as_bytes()).await?;

        self.read_reply().await
    }

    /// Send the `CONNECT` command and wait for the reply.
    pub async fn send_connect_command(&mut self, via_tag: &str) -> Result<()> {
        self.send_command("CONNECT", Some(via_tag)).await
    }

    /// Send the `PASS` command and wait for the reply.
    pub async fn send_pass_command(&mut self, password: &str) -> Result<()> {
        self.send_command("PASS", Some(password)).await
    }

    /// Send the `CHANNEL` command and wait for the reply.
    pub async fn send_channel_command(&mut self, channel: u8) -> Result<()> {
        self.send_command("CHANNEL", Some(&format!("{}", channel)))
            .await
    }

    /// Send the `DATA` command and wait for the reply.
    ///
    /// This function returns the underlying `TcpStream` since the handshake is complete
    /// after sending this command.
    pub async fn send_data_command(mut self) -> Result<TcpStream> {
        self.send_command("DATA", None).await?;
        Ok(self.stream)
    }

    /// Send the `QUIT` command and wait for the reply.
    pub async fn send_quit_command(mut self) -> Result<()> {
        self.send_command("QUIT", None).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use async_std::net::{SocketAddr, TcpListener, TcpStream};

    use crate::tcp_server_handshake::TcpServerHandshake;

    use super::*;

    #[test]
    fn test() -> Result<()> {
        async_std::task::block_on(async {
            let addr = "127.0.0.1:0".parse::<SocketAddr>()?;
            let listener = TcpListener::bind(&addr).await?;
            let addr = listener.local_addr()?;

            let server_future = async_std::task::spawn::<_, Result<()>>(async move {
                let (stream, _) = listener.accept().await?;

                let mut hs = TcpServerHandshake::start(stream).await?;
                hs.receive_connect_command().await?;
                hs.receive_pass_command().await?;
                hs.receive_channel_command().await?;
                let stream = hs.receive_data_command().await?;

                drop(stream);

                Ok(())
            });

            let client_future = async_std::task::spawn::<_, Result<()>>(async move {
                let stream = TcpStream::connect(addr).await?;

                let mut hs = TcpClientHandshake::start(stream).await?;
                hs.send_connect_command("via_tag").await?;
                hs.send_pass_command("password").await?;
                hs.send_channel_command(1).await?;
                let stream = hs.send_data_command().await?;

                drop(stream);

                Ok(())
            });

            server_future.await?;
            client_future.await?;

            Ok(())
        })
    }
}
