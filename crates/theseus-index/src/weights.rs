//! The model's files, read and checked: `model.safetensors` read once, in
//! order, hashing every byte as it goes and copying each tensor into its own
//! buffer, and the tensors kept only if the file's SHA-256 is the one pinned
//! in the crate. Reading instead of mapping keeps a load's RSS from counting
//! the file beside the copies (about 1.06 GB at the spike's peak, against
//! the 530 MiB the tensors hold), and hashing while reading costs no second
//! pass over 547 MB.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use anyhow::{bail, ensure, Context as _};
use candle_core::{Device, Tensor};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::model::NomicConfig;

/// Why a model's files did not load.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("no {0}")]
    Missing(PathBuf),
    /// The file is there, and is not the one the crate pins.
    #[error("{file} is not the pinned file: its SHA-256 is {got}, not {want}")]
    Refused {
        file: PathBuf,
        want: String,
        got: String,
    },
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl LoadError {
    fn io(path: &Path, e: io::Error) -> LoadError {
        if e.kind() == io::ErrorKind::NotFound {
            LoadError::Missing(path.to_path_buf())
        } else {
            LoadError::Other(anyhow::Error::from(e).context(format!("reading {}", path.display())))
        }
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A whole file, and its SHA-256, if that is `want`.
pub fn read_pinned(path: &Path, want: &str) -> Result<Vec<u8>, LoadError> {
    let bytes = std::fs::read(path).map_err(|e| LoadError::io(path, e))?;
    let got = hex(&Sha256::digest(&bytes));
    if got != want {
        return Err(LoadError::Refused {
            file: path.to_path_buf(),
            want: want.to_string(),
            got,
        });
    }
    Ok(bytes)
}

/// A file's SHA-256, streamed.
pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut f = File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finalize()))
}

/// How many bytes are read at a time (a multiple of 4: f32s never straddle
/// two reads).
const READ: usize = 1 << 20;

/// The f32 tensors of a safetensors file, if its SHA-256 is `want`. A file
/// that does not even read as a checkpoint is named by its hash too.
pub fn read_safetensors(path: &Path, want: &str) -> Result<HashMap<String, Tensor>, LoadError> {
    let mut f = File::open(path).map_err(|e| LoadError::io(path, e))?;
    let mut h = Sha256::new();
    let refused = |got: String| LoadError::Refused {
        file: path.to_path_buf(),
        want: want.to_string(),
        got,
    };
    match read_tensors(&mut f, &mut h) {
        Ok(tensors) => {
            let got = hex(&h.finalize());
            if got != want {
                return Err(refused(got));
            }
            Ok(tensors)
        }
        Err(e) => match sha256_file(path) {
            Ok(got) if got != want => Err(refused(got)),
            _ => Err(LoadError::Other(
                e.context(format!("reading {}", path.display())),
            )),
        },
    }
}

fn read_tensors(f: &mut File, h: &mut Sha256) -> anyhow::Result<HashMap<String, Tensor>> {
    let mut len8 = [0u8; 8];
    f.read_exact(&mut len8)?;
    h.update(len8);
    let n = u64::from_le_bytes(len8);
    ensure!(n <= 100 << 20, "a header of {n} bytes");
    let mut header = vec![0u8; n as usize];
    f.read_exact(&mut header)?;
    h.update(&header);
    let header: serde_json::Map<String, Value> =
        serde_json::from_slice(&header).context("the header is not JSON")?;
    let mut infos = Vec::new();
    for (name, v) in header {
        if name == "__metadata__" {
            continue;
        }
        let dtype = v["dtype"].as_str().unwrap_or_default();
        ensure!(
            dtype == "F32",
            "{name} is {dtype}: only f32 checkpoints load"
        );
        let shape: Vec<usize> = v["shape"]
            .as_array()
            .context("no shape")?
            .iter()
            .map(|d| d.as_u64().map(|d| d as usize).context("a bad dimension"))
            .collect::<anyhow::Result<_>>()?;
        let off = v["data_offsets"].as_array().context("no data_offsets")?;
        let (begin, end) = (
            off.first()
                .and_then(Value::as_u64)
                .context("a bad offset")?,
            off.get(1).and_then(Value::as_u64).context("a bad offset")?,
        );
        let elems: usize = shape.iter().product();
        ensure!(
            end >= begin && (end - begin) as usize == elems * 4,
            "{name}'s bytes do not match its shape"
        );
        infos.push((begin, end, name, shape));
    }
    infos.sort();
    let mut at = 0u64;
    let mut buf = vec![0u8; READ];
    let mut out = HashMap::with_capacity(infos.len());
    for (begin, end, name, shape) in infos {
        ensure!(begin >= at, "{name} overlaps the tensor before it");
        skip(f, h, begin - at, &mut buf)?;
        let mut data: Vec<f32> = Vec::with_capacity(((end - begin) / 4) as usize);
        let mut left = (end - begin) as usize;
        while left > 0 {
            let take = left.min(READ);
            f.read_exact(&mut buf[..take])?;
            h.update(&buf[..take]);
            let (quads, _) = buf[..take].as_chunks::<4>();
            data.extend(quads.iter().map(|b| f32::from_le_bytes(*b)));
            left -= take;
        }
        out.insert(name, Tensor::from_vec(data, shape, &Device::Cpu)?);
        at = end;
    }
    // Anything after the last tensor is hashed too.
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(out)
}

fn skip(f: &mut File, h: &mut Sha256, mut n: u64, buf: &mut [u8]) -> io::Result<()> {
    while n > 0 {
        let take = n.min(buf.len() as u64) as usize;
        f.read_exact(&mut buf[..take])?;
        h.update(&buf[..take]);
        n -= take as u64;
    }
    Ok(())
}

/// Write tensors as a safetensors file (tests, and the live check's wrong
/// file).
pub fn write_safetensors(tensors: &HashMap<String, Tensor>, path: &Path) -> anyhow::Result<()> {
    candle_core::safetensors::save(tensors, path)?;
    Ok(())
}

/// SplitMix64: the tiny model's weights, the same on every machine.
#[derive(Debug, Clone)]
pub struct SplitMix(pub u64);

impl SplitMix {
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform in [-1, 1).
    pub fn unit(&mut self) -> f32 {
        ((self.next_u64() >> 40) as f32 / (1u64 << 23) as f32) - 1.0
    }
}

/// Seeded weights for a model of `c`'s shape: every matrix uniform in ±1/√fan-in,
/// the embedding tables ±0.1, the layer norms' weights near 1 and biases
/// near 0. Deterministic in `seed`.
pub fn seeded(c: &NomicConfig, seed: u64) -> anyhow::Result<HashMap<String, Tensor>> {
    let mut rng = SplitMix(seed);
    let mut out = HashMap::new();
    for (name, shape) in c.tensors() {
        let n: usize = shape.iter().product();
        let (base, spread) = if name.ends_with("norm1.weight")
            || name.ends_with("norm2.weight")
            || name == "emb_ln.weight"
        {
            (1.0, 0.1)
        } else if name.ends_with(".bias") || name.starts_with("embeddings.") {
            (0.0, 0.1)
        } else {
            (0.0, 1.0 / (shape[1] as f32).sqrt())
        };
        let data: Vec<f32> = (0..n).map(|_| base + spread * rng.unit()).collect();
        out.insert(name, Tensor::from_vec(data, shape, &Device::Cpu)?);
    }
    if out.is_empty() {
        bail!("a model with no tensors");
    }
    Ok(out)
}
