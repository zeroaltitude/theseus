//! The model's files, read and checked: `model.safetensors` mapped, each
//! tensor copied out of the map into its own buffer a window at a time, in
//! order, and the tensors kept only if the file's SHA-256 is the one pinned
//! in the crate. Each window's pages leave the resident set once copied, so
//! a load's RSS never counts the file beside the copies (about 1.06 GB at the
//! spike's peak, against the 530 MiB the tensors hold), and the hash is taken
//! while copying, once per file signature (`mapped`, theseus-agqn): a load
//! after the idle unload copies, and hashes nothing.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use anyhow::{bail, ensure, Context as _};
use candle_core::{Device, Tensor};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::mapped;
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

/// The f32 tensors of a safetensors file, if its SHA-256 is `want`: the
/// file mapped and copied a window at a time, and hashed only when this
/// process has not hashed it at its signature before (`mapped`,
/// theseus-agqn). A file that does not even read as a checkpoint is named by
/// its hash too.
pub fn read_safetensors(path: &Path, want: &str) -> Result<HashMap<String, Tensor>, LoadError> {
    let f = File::open(path).map_err(|e| LoadError::io(path, e))?;
    let sig = mapped::Sig::of(&f.metadata().map_err(|e| LoadError::io(path, e))?);
    let refused = |got: String| LoadError::Refused {
        file: path.to_path_buf(),
        want: want.to_string(),
        got,
    };
    let known = mapped::known(path, &sig);
    if let Some(got) = &known {
        if got != want {
            return Err(refused(got.clone()));
        }
    }
    let map = mapped::Map::of(&f).map_err(|e| LoadError::io(path, e))?;
    let mut h = known.is_none().then(Sha256::new);
    match read_tensors(&map, h.as_mut()) {
        Ok(tensors) => {
            if let Some(h) = h {
                let got = hex(&h.finalize());
                mapped::remember(path, sig, &got);
                if got != want {
                    return Err(refused(got));
                }
            }
            Ok(tensors)
        }
        Err(e) => {
            let got = known.unwrap_or_else(|| {
                let mut h = Sha256::new();
                feed(&map, Some(&mut h), 0, map.bytes().len());
                let got = hex(&h.finalize());
                mapped::remember(path, sig, &got);
                got
            });
            if got == want {
                Err(LoadError::Other(
                    e.context(format!("reading {}", path.display())),
                ))
            } else {
                Err(refused(got))
            }
        }
    }
}

/// `[at, at + len)` of the map hashed into `h` (when there is one) a window
/// at a time, each window's pages released after.
fn feed(map: &mapped::Map, mut h: Option<&mut Sha256>, at: usize, len: usize) {
    let bytes = map.bytes();
    let mut i = at;
    while i < at + len {
        let n = (at + len - i).min(mapped::WINDOW);
        if let Some(h) = h.as_deref_mut() {
            h.update(&bytes[i..i + n]);
        }
        map.release(i, n);
        i += n;
    }
}

/// The tensors, in the file's order, each copied out of the map a window at
/// a time (hashed into `h` as it goes, and every byte between them too).
fn read_tensors(
    map: &mapped::Map,
    mut h: Option<&mut Sha256>,
) -> anyhow::Result<HashMap<String, Tensor>> {
    let bytes = map.bytes();
    ensure!(bytes.len() >= 8, "shorter than its header's length");
    let n = u64::from_le_bytes(bytes[..8].try_into()?);
    ensure!(n <= 100 << 20, "a header of {n} bytes");
    let base = 8 + n as usize;
    ensure!(bytes.len() >= base, "shorter than its header");
    let header: serde_json::Map<String, Value> =
        serde_json::from_slice(&bytes[8..base]).context("the header is not JSON")?;
    feed(map, h.as_deref_mut(), 0, base);
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
        ensure!(
            base as u64 + end <= bytes.len() as u64,
            "{name} runs past the file's end"
        );
        infos.push((begin as usize, end as usize, name, shape));
    }
    infos.sort();
    let mut at = 0usize;
    let mut out = HashMap::with_capacity(infos.len());
    for (begin, end, name, shape) in infos {
        ensure!(begin >= at, "{name} overlaps the tensor before it");
        feed(map, h.as_deref_mut(), base + at, begin - at);
        let mut data: Vec<f32> = Vec::with_capacity((end - begin) / 4);
        let mut i = base + begin;
        while i < base + end {
            let n = (base + end - i).min(mapped::WINDOW);
            let window = &bytes[i..i + n];
            if let Some(h) = h.as_deref_mut() {
                h.update(window);
            }
            let (quads, _) = window.as_chunks::<4>();
            data.extend(quads.iter().map(|b| f32::from_le_bytes(*b)));
            map.release(i, n);
            i += n;
        }
        out.insert(name, Tensor::from_vec(data, shape, &Device::Cpu)?);
        at = end;
    }
    // Anything after the last tensor is hashed too.
    feed(map, h, base + at, bytes.len() - base - at);
    Ok(out)
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
