use std::fs::File;

use async_std::{
    net::{SocketAddr, TcpStream},
};

use resol_vbus::{DataSet, RecordingWriter};

use async_resol_vbus::{LiveDataStream, Result, TcpClientHandshake};

fn main() -> Result<()> {
    async_std::task::block_on(async {
        // Create an recording file and hand it to a `RecordingWriter`
        let file = File::create("test.vbus")?;
        let mut rw = RecordingWriter::new(file);

        // Parse the address of the DL2 to connect to
        let addr = "192.168.5.217:7053".parse::<SocketAddr>()?;

        let stream = TcpStream::connect(addr).await?;

        let mut hs = TcpClientHandshake::start(stream).await?;
        hs.send_pass_command("vbus").await?;
        let stream = hs.send_data_command().await?;

        let (reader, writer) = (&stream, &stream);

        let mut stream = LiveDataStream::new(reader, writer, 0, 0x0020);

        while let Some(data) = stream.receive_any_data(60000).await? {
            println!("{}", data.id_string());

            // Add `Data` value into `DataSet` to be stored
            let mut data_set = DataSet::new();
            data_set.timestamp = data.as_ref().timestamp;
            data_set.add_data(data);

            // Write the `DataSet` into the `RecordingWriter` for permanent storage
            rw.write_data_set(&data_set)
                .expect("Unable to write data set");
        }

        Ok(())
    })
}
