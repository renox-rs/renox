//! A malware scan with ClamAV's daemon (`clamd`), over its `INSTREAM`
//! command. No ClamAV library is linked: the file is streamed to the daemon
//! on a TCP or Unix socket.

use std::time::Duration;

use futures_util::future::BoxFuture;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::Upload;
use super::pipeline::{StepOutcome, UploadContext, UploadStep};
use crate::Result;

/// Where `clamd` listens.
#[derive(Clone, Debug)]
enum Address {
    Tcp(String),
    #[cfg(unix)]
    Unix(std::path::PathBuf),
}

/// An [`UploadStep`] that scans every file with `clamd`.
///
/// ```
/// # use renox::prelude::*;
/// use renox::upload::{ClamAv, Pipeline};
/// use std::time::Duration;
///
/// # let _ =
/// App::new().uploads(Pipeline::new().step(
///     ClamAv::tcp("127.0.0.1:3310").timeout(Duration::from_secs(20)),
/// ))
/// # ;
/// ```
///
/// A file with a signature is refused ("contains malware"). **It fails
/// closed**: when the daemon can't be reached, answers with an error (a file
/// over its `StreamMaxLength`, say) or doesn't answer within the timeout
/// (10 seconds unless [`timeout`](ClamAv::timeout) says otherwise), the file
/// is refused too, and the reason is logged. [`fail_open`](ClamAv::fail_open)
/// lets such files through instead, for apps that prefer availability.
#[derive(Clone, Debug)]
pub struct ClamAv {
    address: Address,
    timeout: Duration,
    fail_open: bool,
}

impl ClamAv {
    /// A daemon on a TCP address, e.g. `127.0.0.1:3310` (`TCPSocket` in
    /// `clamd.conf`).
    pub fn tcp(address: impl Into<String>) -> Self {
        Self::at(Address::Tcp(address.into()))
    }

    /// A daemon on a Unix socket, e.g. `/var/run/clamav/clamd.ctl`
    /// (`LocalSocket` in `clamd.conf`).
    #[cfg(unix)]
    pub fn unix(path: impl Into<std::path::PathBuf>) -> Self {
        Self::at(Address::Unix(path.into()))
    }

    fn at(address: Address) -> Self {
        Self {
            address,
            timeout: Duration::from_secs(10),
            fail_open: false,
        }
    }

    /// How long connecting, sending and waiting for the verdict may take in
    /// all, per file (default 10 seconds).
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Lets files through when they couldn't be scanned (the daemon is down,
    /// too slow, or refused the file's size). The default is to refuse them.
    pub fn fail_open(mut self) -> Self {
        self.fail_open = true;
        self
    }

    /// The daemon's reply for `bytes`.
    async fn scan(&self, bytes: &[u8]) -> std::io::Result<String> {
        match &self.address {
            Address::Tcp(address) => {
                let stream = tokio::net::TcpStream::connect(address).await?;
                instream(stream, bytes).await
            }
            #[cfg(unix)]
            Address::Unix(path) => {
                let stream = tokio::net::UnixStream::connect(path).await?;
                instream(stream, bytes).await
            }
        }
    }
}

/// What `clamd` said.
#[derive(Debug, PartialEq)]
enum Verdict {
    Clean,
    Infected(String),
    Failed(String),
}

fn verdict(reply: &str) -> Verdict {
    let reply = reply.trim_matches(|c: char| c == '\0' || c.is_whitespace());
    if reply.ends_with(" OK") {
        Verdict::Clean
    } else if let Some(found) = reply.strip_suffix(" FOUND") {
        let name = found.rsplit_once(": ").map_or(found, |(_, name)| name);
        Verdict::Infected(name.to_owned())
    } else {
        Verdict::Failed(reply.to_owned())
    }
}

/// Sends `zINSTREAM`, the bytes in length-prefixed chunks and the
/// zero-length end; returns the reply.
async fn instream<S>(mut stream: S, bytes: &[u8]) -> std::io::Result<String>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let sent = async {
        stream.write_all(b"zINSTREAM\0").await?;
        for chunk in bytes.chunks(64 * 1024) {
            stream
                .write_all(&(chunk.len() as u32).to_be_bytes())
                .await?;
            stream.write_all(chunk).await?;
        }
        stream.write_all(&0u32.to_be_bytes()).await?;
        stream.flush().await
    }
    .await;
    let mut reply = Vec::new();
    // The daemon may answer (and hang up) before the end, e.g. when the file is
    // over its size limit: read its words, whatever happened to the writing.
    let read = stream.read_to_end(&mut reply).await;
    if reply.is_empty() {
        sent?;
        read?;
    }
    Ok(String::from_utf8_lossy(&reply).into_owned())
}

impl UploadStep for ClamAv {
    fn process<'a>(
        &'a self,
        file: &'a mut Upload,
        ctx: &'a UploadContext,
    ) -> BoxFuture<'a, Result<StepOutcome>> {
        Box::pin(async move {
            let reply = tokio::time::timeout(self.timeout, self.scan(file.bytes())).await;
            let failure = match reply {
                Ok(Ok(reply)) => match verdict(&reply) {
                    Verdict::Clean => return Ok(StepOutcome::Keep),
                    Verdict::Infected(name) => {
                        tracing::warn!(
                            field = ctx.field(),
                            file = file.file_name(),
                            signature = name,
                            "clamd found malware in an upload"
                        );
                        return Ok(StepOutcome::Reject(
                            "The file was refused: it contains malware.".into(),
                        ));
                    }
                    Verdict::Failed(reply) => format!("clamd answered `{reply}`"),
                },
                Ok(Err(err)) => format!("clamd could not be reached: {err}"),
                Err(_) => format!("clamd didn't answer within {:?}", self.timeout),
            };
            tracing::warn!(
                field = ctx.field(),
                file = file.file_name(),
                "the upload was not scanned: {failure}"
            );
            if self.fail_open {
                Ok(StepOutcome::Keep)
            } else {
                Ok(StepOutcome::Reject(
                    "The file could not be checked for malware. Try again later.".into(),
                ))
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_clamds_replies() {
        assert_eq!(verdict("stream: OK\0"), Verdict::Clean);
        assert_eq!(
            verdict("stream: Eicar-Test-Signature FOUND\0"),
            Verdict::Infected("Eicar-Test-Signature".into())
        );
        assert_eq!(
            verdict("INSTREAM size limit exceeded. ERROR\0"),
            Verdict::Failed("INSTREAM size limit exceeded. ERROR".into())
        );
        assert_eq!(verdict(""), Verdict::Failed(String::new()));
    }
}
