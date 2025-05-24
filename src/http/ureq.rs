use crate::{Error, Result};
use pipe::{PipeBufWriter, PipeReader};
use std::fmt::{self, Debug};
use std::io::{Error as IoError, Read, Result as IoResult, Write};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::thread::spawn;

pub struct HttpWriter {
    write: PipeBufWriter,
    rx: Mutex<Receiver<Result<()>>>,
}

/// A wrapper for the read end of the pipe that sniches on when data is first read
/// by sending `Ok(())` down tx.
///
/// This is used so that we can block the code making the put request until ethier:
/// a) the data is tried to be read, or
/// b) the request fails before trying to send the payload (bad hostname, invalid auth, etc)
struct SnitchingReader {
    read: PipeReader,
    connected: bool,
    tx: SyncSender<Result<()>>,
}

impl Read for SnitchingReader {
    fn read(&mut self, buffer: &mut [u8]) -> IoResult<usize> {
        if !self.connected {
            self.tx.send(Ok(())).map_err(IoError::other)?;
            self.connected = true;
        }
        self.read.read(buffer)
    }
}

impl HttpWriter {
    pub fn new(url: &str, size: Option<u64>) -> Result<Self> {
        let (read, write) = pipe::pipe_buffered();

        let mut req = ureq::put(url);
        if let Some(size) = size {
            req = req.header("content-length", &size.to_string());
        }

        let (done_tx, rx) = sync_channel(0);
        let mut snitch = SnitchingReader {
            read,
            connected: false,
            tx: done_tx.clone(),
        };

        spawn(move || {
            let mut buf = vec![];
            snitch.read_to_end(&mut buf).ok();

            done_tx
                .send(req.send(buf).map(|_| ()).map_err(|e| e.into()))
                .unwrap();
        });

        // either Ok(()) if the other thread started reading or the connection error
        rx.recv().unwrap()?;
        let rx = Mutex::new(rx);
        Ok(HttpWriter { write, rx })
    }

    pub fn finish(self) -> Result<()> {
        drop(self.write);
        self.rx
            .try_lock()
            .expect("clio HttpWriter lock should one ever be taken once while dropping")
            .recv()
            .unwrap()?;
        Ok(())
    }
}

impl Write for HttpWriter {
    fn write(&mut self, buffer: &[u8]) -> IoResult<usize> {
        self.write.write(buffer)
    }
    fn flush(&mut self) -> IoResult<()> {
        self.write.flush()
    }
}

impl fmt::Debug for HttpWriter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpWriter").finish()
    }
}

pub struct HttpReader {
    length: Option<u64>,
    #[cfg(feature = "clap-parse")]
    read: Mutex<Box<dyn Read + Send>>,
    #[cfg(not(feature = "clap-parse"))]
    read: Box<dyn Read + Send>,
}

impl HttpReader {
    pub fn new(url: &str) -> Result<Self> {
        let resp = ureq::get(url).call()?;

        let length = resp
            .headers()
            .get("Content-Length")
            .and_then(|x| x.to_str().ok())
            .and_then(|x| x.parse::<u64>().ok());

        let (_, body) = resp.into_parts();

        Ok(HttpReader {
            length,
            #[cfg(not(feature = "clap-parse"))]
            read: Box::new(body.into_reader()),
            #[cfg(feature = "clap-parse")]
            read: Mutex::new(Box::new(body.into_reader())),
        })
    }

    pub fn len(&self) -> Option<u64> {
        self.length
    }
}

impl Read for HttpReader {
    #[cfg(not(feature = "clap-parse"))]
    fn read(&mut self, buffer: &mut [u8]) -> IoResult<usize> {
        self.read.read(buffer)
    }

    #[cfg(feature = "clap-parse")]
    fn read(&mut self, buffer: &mut [u8]) -> IoResult<usize> {
        self.read
            .lock()
            .map_err(|_| IoError::other("Error locking HTTP reader"))?
            .read(buffer)
    }
}

impl Debug for HttpReader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpReader").finish()
    }
}

impl From<ureq::Error> for Error {
    fn from(err: ureq::Error) -> Self {
        match err {
            ureq::Error::StatusCode(code) => Error::Http {
                code,
                message: err.to_string(),
            },
            _ => Error::Http {
                code: 499,
                message: err.to_string(),
            },
        }
    }
}
