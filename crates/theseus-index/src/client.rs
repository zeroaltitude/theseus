//! A client for the tender's socket: the binary's, the tests', and the shape
//! of the core's (row 51), which adds the deadline to every call.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use anyhow::{bail, Context as _};
use serde::{de::DeserializeOwned, Serialize};
use theseus_protocol::{Id, Request, Response};

pub struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    next: u64,
}

impl Client {
    /// Connect, with `timeout` on every read and write after.
    pub fn connect(path: &Path, timeout: Duration) -> anyhow::Result<Self> {
        let s = UnixStream::connect(path)
            .with_context(|| format!("connecting to the index tender at {}", path.display()))?;
        s.set_read_timeout(Some(timeout))?;
        s.set_write_timeout(Some(timeout))?;
        Ok(Self {
            reader: BufReader::new(s.try_clone()?),
            writer: s,
            next: 1,
        })
    }

    pub fn call<T: DeserializeOwned>(
        &mut self,
        method: &str,
        params: impl Serialize,
    ) -> anyhow::Result<T> {
        let id = self.next;
        self.next += 1;
        let mut line = serde_json::to_vec(&Request::new(Id::Num(id), method, params))?;
        line.push(b'\n');
        self.writer.write_all(&line)?;
        let mut answer = String::new();
        if self.reader.read_line(&mut answer)? == 0 {
            bail!("the index tender closed the connection");
        }
        let resp: Response = serde_json::from_str(&answer)?;
        if resp.id != Id::Num(id) {
            bail!("an answer to another request ({:?})", resp.id);
        }
        if let Some(e) = resp.error {
            bail!("{} ({})", e.message, e.code);
        }
        Ok(serde_json::from_value(resp.result.unwrap_or_default())?)
    }
}
