//! The cockpit's files as the binary embeds them (theseus-rwpbg.5): the cockpit's build writes `<file>.br` and
//! `<file>.gz` beside each file it can shrink (`cockpit/compress.ts`), and only those and the fonts, which are packed
//! already, are embedded. A request for `x` is answered with `x.br` when its `Accept-Encoding` takes brotli, else
//! `x.gz` when it takes gzip, else the brotli copy decoded; always with `x`'s own `Content-Type` and `Vary:
//! Accept-Encoding`. There is no `Range`: the web UI has never answered one, so an encoded body has no range to break.

use std::borrow::Cow;
use std::io::Read;

use axum::{
    body::Body,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use rust_embed::Embed;

/// Whether a path names one of the compressed copies, which are never served as themselves.
fn is_copy(path: &str) -> bool {
    path.ends_with(".br") || path.ends_with(".gz")
}

/// Whether the cockpit has a file at `path`, in any of its embedded forms.
pub fn exists<E: Embed>(path: &str) -> bool {
    !is_copy(path) && (E::get(path).is_some() || E::get(&format!("{path}.br")).is_some())
}

/// Whether an `Accept-Encoding` takes `coding`: it lists the coding (or `*`) and does not give it `q=0`.
fn accepts(accept: &str, coding: &str) -> bool {
    accept.split(',').any(|item| {
        let mut parts = item.split(';');
        let name = parts.next().unwrap_or("").trim();
        let refused = parts.any(|p| {
            p.trim()
                .strip_prefix("q=")
                .is_some_and(|q| q.trim().parse::<f32>() == Ok(0.0))
        });
        (name.eq_ignore_ascii_case(coding) || name == "*") && !refused
    })
}

/// The bytes of `path` and the coding they are in (none: as the build wrote them).
fn pick<E: Embed>(path: &str, accept: &str) -> Option<(Cow<'static, [u8]>, Option<&'static str>)> {
    if let Some(f) = E::get(path).filter(|_| !is_copy(path)) {
        return Some((f.data, None));
    }
    for (coding, ext) in [("br", "br"), ("gzip", "gz")] {
        if accepts(accept, coding) {
            if let Some(f) = E::get(&format!("{path}.{ext}")) {
                return Some((f.data, Some(coding)));
            }
        }
    }
    let mut plain = Vec::new();
    let packed = E::get(&format!("{path}.br"))?;
    brotli_decompressor::Decompressor::new(packed.data.as_ref(), 16 * 1024)
        .read_to_end(&mut plain)
        .ok()?;
    Some((Cow::Owned(plain), None))
}

/// The response for `path`: its bytes in the best coding the request takes, or a 404 saying `missing`.
pub fn respond<E: Embed>(path: &str, request: &HeaderMap, missing: &'static str) -> Response {
    let accept = request
        .get(header::ACCEPT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let Some((data, coding)) = pick::<E>(path, accept) else {
        return (StatusCode::NOT_FOUND, missing).into_response();
    };
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let cache = if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    let mut r = (
        [
            (header::CONTENT_TYPE, mime.as_ref()),
            (header::CACHE_CONTROL, cache),
        ],
        Body::from(data.into_owned()),
    )
        .into_response();
    if !path.ends_with(".woff2") {
        r.headers_mut()
            .insert(header::VARY, "Accept-Encoding".parse().unwrap());
    }
    if let Some(c) = coding {
        r.headers_mut()
            .insert(header::CONTENT_ENCODING, c.parse().unwrap());
    }
    r
}

#[cfg(test)]
mod tests {
    use super::super::{Cockpit, COCKPIT_MISSING};
    use super::*;
    use std::path::{Path, PathBuf};

    fn dist() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("cockpit/dist")
    }

    /// The files the cockpit's build wrote, as names under `dist`, its compressed copies left out. `None` when the
    /// cockpit is not built, as the other tests of `/` allow.
    fn originals() -> Option<Vec<String>> {
        let root = dist();
        let mut out = Vec::new();
        let mut dirs = vec![root.clone()];
        while let Some(d) = dirs.pop() {
            for e in std::fs::read_dir(&d).ok()? {
                let p = e.ok()?.path();
                if p.is_dir() {
                    dirs.push(p);
                } else if !is_copy(p.to_str()?) {
                    out.push(p.strip_prefix(&root).ok()?.to_str()?.to_string());
                }
            }
        }
        out.sort();
        Some(out)
    }

    fn embedded(name: &str) -> Vec<u8> {
        Cockpit::get(name)
            .unwrap_or_else(|| {
                panic!("{name} is not embedded: rebuild the cockpit (`npm run build` in cockpit/)")
            })
            .data
            .into_owned()
    }

    /// Every file of the cockpit's build is embedded and decodes to its bytes: a font as it is, anything else as a
    /// brotli copy and a gzip copy. A dist built before the copies existed fails here, naming its first file.
    #[test]
    fn every_embedded_asset_decodes_to_its_dist_file() {
        let Some(files) = originals() else { return };
        assert!(!files.is_empty());
        for name in files {
            let want = std::fs::read(dist().join(&name)).unwrap();
            if name.ends_with(".woff2") {
                assert_eq!(embedded(&name), want, "{name}");
                continue;
            }
            let mut br = Vec::new();
            brotli_decompressor::Decompressor::new(
                embedded(&format!("{name}.br")).as_slice(),
                4096,
            )
            .read_to_end(&mut br)
            .unwrap();
            assert!(br == want, "{name}.br decodes to other bytes");
            let mut gz = Vec::new();
            flate2::read::GzDecoder::new(embedded(&format!("{name}.gz")).as_slice())
                .read_to_end(&mut gz)
                .unwrap();
            assert!(gz == want, "{name}.gz decodes to other bytes");
        }
    }

    fn ask(path: &str, accept: Option<&str>) -> Response {
        let mut h = HeaderMap::new();
        if let Some(a) = accept {
            h.insert(header::ACCEPT_ENCODING, a.parse().unwrap());
        }
        respond::<Cockpit>(path, &h, COCKPIT_MISSING)
    }

    fn coding(r: &Response) -> Option<&str> {
        r.headers()
            .get(header::CONTENT_ENCODING)
            .map(|v| v.to_str().unwrap())
    }

    async fn body(r: Response) -> Vec<u8> {
        axum::body::to_bytes(r.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec()
    }

    #[tokio::test]
    async fn the_cockpit_serves_br_then_gzip_then_identity() {
        let Some(files) = originals() else { return };
        let name = files.iter().find(|n| n.ends_with(".js")).unwrap();
        let want = std::fs::read(dist().join(name)).unwrap();
        for (accept, coded) in [
            (Some("br, gzip"), Some("br")),
            (Some("gzip, deflate, br"), Some("br")),
            (Some("gzip"), Some("gzip")),
            (Some("br;q=0, gzip"), Some("gzip")),
            (Some("*"), Some("br")),
            (Some("identity"), None),
            (Some("br;q=0, gzip;q=0"), None),
            (None, None),
        ] {
            let r = ask(name, accept);
            assert_eq!(r.status(), StatusCode::OK, "{accept:?}");
            assert_eq!(coding(&r), coded, "{accept:?}");
            // The bytes sent are the embedded copy of that coding, or the file itself.
            let sent = match coded {
                Some("br") => embedded(&format!("{name}.br")),
                Some(_) => embedded(&format!("{name}.gz")),
                None => want.clone(),
            };
            assert!(body(r).await == sent, "{accept:?}");
        }
    }

    #[tokio::test]
    async fn vary_and_content_type_are_set() {
        let Some(files) = originals() else { return };
        for (suffix, mime) in [
            (".js", "text/javascript"),
            (".css", "text/css"),
            (".svg", "image/svg+xml"),
        ] {
            let name = files.iter().find(|n| n.ends_with(suffix)).unwrap();
            for accept in [Some("br"), Some("gzip"), None] {
                let r = ask(name, accept);
                assert!(
                    r.headers()[header::CONTENT_TYPE]
                        .to_str()
                        .unwrap()
                        .starts_with(mime),
                    "{name}: {:?}",
                    r.headers()
                );
                assert_eq!(
                    r.headers()[header::VARY],
                    "Accept-Encoding",
                    "{name} {accept:?}"
                );
            }
        }
        // A font is packed already: sent as it is, and the same to every client.
        let font = files.iter().find(|n| n.ends_with(".woff2")).unwrap();
        let r = ask(font, Some("br, gzip"));
        assert_eq!(r.headers()[header::CONTENT_TYPE], "font/woff2");
        assert!(coding(&r).is_none() && r.headers().get(header::VARY).is_none());
        // The copies are not addressable as files, and a missing page is a 404.
        assert!(!exists::<Cockpit>(&format!("{}.br", files[0])));
        assert_eq!(ask("no/such/file.js", None).status(), StatusCode::NOT_FOUND);
    }
}
