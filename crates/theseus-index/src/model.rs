//! Nomic BERT (nomic-embed-text-v1.5) on candle: the 29a spike's port
//! (bit-identical to candle-transformers' `nomic_bert`), cut to this
//! checkpoint (post-norm, no biases, non-interleaved rotary, SwiGLU), at f32.
//! Its constants are a [`NomicConfig`], so the tests build a tiny model of
//! the same architecture from the same code, with seeded weights.

use candle_core::{DType, Module, Result, Tensor};
use candle_nn::{Embedding, LayerNorm, VarBuilder};

/// The architecture's sizes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NomicConfig {
    pub hidden: usize,
    pub heads: usize,
    /// The MLP's inner width (SwiGLU's two gates, each this wide).
    pub inner: usize,
    pub layers: usize,
    pub vocab: usize,
    pub types: usize,
    pub eps: f64,
    pub rope_base: f32,
    /// The longest sequence the rotary tables cover: the embedder's cap.
    pub max_pos: usize,
}

impl NomicConfig {
    /// nomic-embed-text-v1.5's `config.json`, with the rotary tables cut to
    /// the embedder's 512 tokens.
    pub const V1_5: NomicConfig = NomicConfig {
        hidden: 768,
        heads: 12,
        inner: 3072,
        layers: 12,
        vocab: 30528,
        types: 2,
        eps: 1e-12,
        rope_base: 1000.0,
        max_pos: crate::embedder::MAX_TOKENS,
    };

    pub fn head_dim(&self) -> usize {
        self.hidden / self.heads
    }

    /// Every tensor the model reads, with its shape: the checkpoint's names.
    pub fn tensors(&self) -> Vec<(String, Vec<usize>)> {
        let (h, i) = (self.hidden, self.inner);
        let mut out = vec![
            (
                "embeddings.word_embeddings.weight".into(),
                vec![self.vocab, h],
            ),
            (
                "embeddings.token_type_embeddings.weight".into(),
                vec![self.types, h],
            ),
            ("emb_ln.weight".into(), vec![h]),
            ("emb_ln.bias".into(), vec![h]),
        ];
        for l in 0..self.layers {
            let p = format!("encoder.layers.{l}");
            out.extend([
                (format!("{p}.attn.Wqkv.weight"), vec![3 * h, h]),
                (format!("{p}.attn.out_proj.weight"), vec![h, h]),
                (format!("{p}.mlp.fc11.weight"), vec![i, h]),
                (format!("{p}.mlp.fc12.weight"), vec![i, h]),
                (format!("{p}.mlp.fc2.weight"), vec![h, i]),
                (format!("{p}.norm1.weight"), vec![h]),
                (format!("{p}.norm1.bias"), vec![h]),
                (format!("{p}.norm2.weight"), vec![h]),
                (format!("{p}.norm2.bias"), vec![h]),
            ]);
        }
        out
    }
}

/// A dense linear without bias, `(out, in)`.
struct Linear(Tensor);

impl Linear {
    fn load(vb: VarBuilder, out: usize, inp: usize) -> Result<Self> {
        Ok(Self(vb.get((out, inp), "weight")?))
    }

    /// `x` is `(batch, seq, in)`; one 2-D matmul over all its rows.
    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let (b, s, i) = x.dims3()?;
        x.reshape((b * s, i))?
            .matmul(&self.0.t()?)?
            .reshape((b, s, ()))
    }
}

fn layer_norm(vb: VarBuilder, size: usize, eps: f64) -> Result<LayerNorm> {
    Ok(LayerNorm::new(
        vb.get(size, "weight")?,
        vb.get(size, "bias")?,
        eps,
    ))
}

struct Block {
    wqkv: Linear,
    out_proj: Linear,
    fc11: Linear,
    fc12: Linear,
    fc2: Linear,
    norm1: LayerNorm,
    norm2: LayerNorm,
    heads: usize,
    head_dim: usize,
    hidden: usize,
}

impl Block {
    fn load(vb: VarBuilder, c: &NomicConfig) -> Result<Self> {
        let (h, i) = (c.hidden, c.inner);
        Ok(Self {
            wqkv: Linear::load(vb.pp("attn.Wqkv"), 3 * h, h)?,
            out_proj: Linear::load(vb.pp("attn.out_proj"), h, h)?,
            fc11: Linear::load(vb.pp("mlp.fc11"), i, h)?,
            fc12: Linear::load(vb.pp("mlp.fc12"), i, h)?,
            fc2: Linear::load(vb.pp("mlp.fc2"), h, i)?,
            norm1: layer_norm(vb.pp("norm1"), h, c.eps)?,
            norm2: layer_norm(vb.pp("norm2"), h, c.eps)?,
            heads: c.heads,
            head_dim: c.head_dim(),
            hidden: h,
        })
    }

    fn attention(&self, x: &Tensor, mask: &Tensor, cos: &Tensor, sin: &Tensor) -> Result<Tensor> {
        let (b, s, _) = x.dims3()?;
        // (b, s, 3·h·d) -> (3, b, h, s, d): Wqkv's rows are q, then k, then v, each head-major.
        let qkv = self
            .wqkv
            .forward(x)?
            .reshape((b, s, 3, self.heads, self.head_dim))?
            .permute((2, 0, 3, 1, 4))?;
        let q = candle_nn::rotary_emb::rope(&qkv.get(0)?.contiguous()?, cos, sin)?;
        let k = candle_nn::rotary_emb::rope(&qkv.get(1)?.contiguous()?, cos, sin)?;
        let v = qkv.get(2)?.contiguous()?;
        let scores = (q.matmul(&k.t()?)? * (1.0 / (self.head_dim as f64).sqrt()))?;
        let probs = candle_nn::ops::softmax_last_dim(&scores.broadcast_add(mask)?)?;
        let out = probs
            .matmul(&v)?
            .transpose(1, 2)?
            .contiguous()?
            .reshape((b, s, self.hidden))?;
        self.out_proj.forward(&out)
    }

    /// Post-norm: `h = norm1(x + attn(x))`, then `norm2(h + fc2(fc11(h) · silu(fc12(h))))`.
    fn forward(&self, x: &Tensor, mask: &Tensor, cos: &Tensor, sin: &Tensor) -> Result<Tensor> {
        let h = self
            .norm1
            .forward(&(x + self.attention(x, mask, cos, sin)?)?)?;
        let y = (self.fc11.forward(&h)? * self.fc12.forward(&h)?.silu()?)?;
        self.norm2.forward(&(&h + self.fc2.forward(&y)?)?)
    }
}

pub struct NomicBert {
    word: Embedding,
    token_type0: Tensor,
    emb_ln: LayerNorm,
    layers: Vec<Block>,
    cos: Tensor,
    sin: Tensor,
    max_pos: usize,
}

impl NomicBert {
    /// The model from `vb` (f32 tensors under the checkpoint's names).
    pub fn load(vb: VarBuilder, c: &NomicConfig) -> Result<Self> {
        let word = vb
            .pp("embeddings.word_embeddings")
            .get((c.vocab, c.hidden), "weight")?;
        let token_type0 = vb
            .pp("embeddings.token_type_embeddings")
            .get((c.types, c.hidden), "weight")?
            .get(0)?;
        let layers = (0..c.layers)
            .map(|i| Block::load(vb.pp(format!("encoder.layers.{i}")), c))
            .collect::<Result<Vec<_>>>()?;
        let half = c.head_dim() / 2;
        let inv_freq: Vec<f32> = (0..half)
            .map(|i| 1.0 / c.rope_base.powf(2.0 * i as f32 / c.head_dim() as f32))
            .collect();
        let dev = vb.device();
        let inv_freq = Tensor::new(inv_freq.as_slice(), dev)?.unsqueeze(0)?;
        let pos = Tensor::arange(0u32, c.max_pos as u32, dev)?
            .to_dtype(DType::F32)?
            .unsqueeze(1)?;
        let freqs = pos.matmul(&inv_freq)?;
        Ok(Self {
            word: Embedding::new(word, c.hidden),
            token_type0,
            emb_ln: layer_norm(vb.pp("emb_ln"), c.hidden, c.eps)?,
            layers,
            cos: freqs.cos()?,
            sin: freqs.sin()?,
            max_pos: c.max_pos,
        })
    }

    /// `ids` and `mask` are `(batch, seq)` u32. Returns the last hidden state
    /// `(batch, seq, hidden)`.
    pub fn forward(&self, ids: &Tensor, mask: &Tensor) -> Result<Tensor> {
        Ok(self
            .forward_while(ids, mask, &mut || true)?
            .expect("a forward pass asked to go on runs to its end"))
    }

    /// [`NomicBert::forward`], asking `go_on` before each layer: `None` once
    /// it says no (a query whose caller has gone, theseus-zo1y), so a pass
    /// stops within one layer.
    pub fn forward_while(
        &self,
        ids: &Tensor,
        mask: &Tensor,
        go_on: &mut dyn FnMut() -> bool,
    ) -> Result<Option<Tensor>> {
        let (_, s) = ids.dims2()?;
        if s > self.max_pos {
            candle_core::bail!("{s} tokens, past the model's {}", self.max_pos);
        }
        let x = self.word.forward(ids)?.broadcast_add(&self.token_type0)?;
        let mut x = self.emb_ln.forward(&x)?;
        // 0 where attended, -10000 where padded, shaped to broadcast over heads and queries.
        let mask = ((mask.to_dtype(DType::F32)? - 1.0)? * 1e4)?
            .unsqueeze(1)?
            .unsqueeze(1)?;
        let cos = self.cos.narrow(0, 0, s)?;
        let sin = self.sin.narrow(0, 0, s)?;
        for layer in &self.layers {
            if !go_on() {
                return Ok(None);
            }
            x = layer.forward(&x, &mask, &cos, &sin)?;
        }
        Ok(Some(x))
    }
}
