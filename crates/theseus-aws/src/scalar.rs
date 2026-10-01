//! A scalar's text where the protocols put text: a query string, a URI
//! label, a header, or an XML element.

use aws_smithy_types::date_time::Format;
use aws_smithy_types::DateTime;
use theseus_aws_catalog::{ShapeRef, TimestampFormat};

use crate::value::{self, V};

/// A scalar's text. `default` is the timestamp format of the place it goes
/// (ISO 8601 in a query or a body, RFC 822 in a header), unless the shape
/// names its own.
pub(crate) fn text(shape: ShapeRef<'_>, v: &V<'_>, default: TimestampFormat) -> String {
    match v {
        V::Str(s) => s.clone(),
        V::Bool(b) => b.to_string(),
        V::Int(i) => i.to_string(),
        V::Float(f) => value::float_text(*f),
        V::Time(t) => time(*t, shape.timestamp_format().unwrap_or(default)),
        V::Blob(b) => value::b64(b),
        V::Doc(d) => d.to_string(),
        V::Struct(_) | V::List(_) | V::Map(_) => String::new(),
    }
}

/// A timestamp in one of the protocols' three formats.
pub(crate) fn time(t: DateTime, format: TimestampFormat) -> String {
    match format {
        TimestampFormat::Iso8601 => t
            .fmt(Format::DateTime)
            .unwrap_or_else(|_| t.secs().to_string()),
        TimestampFormat::Rfc822 => t
            .fmt(Format::HttpDate)
            .unwrap_or_else(|_| t.secs().to_string()),
        TimestampFormat::UnixTimestamp => epoch(t),
    }
}

/// Epoch seconds: whole when the time is, else to the millisecond.
pub(crate) fn epoch(t: DateTime) -> String {
    if t.subsec_nanos() == 0 {
        t.secs().to_string()
    } else {
        let s = format!("{:.3}", t.as_secs_f64());
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_in_each_format() {
        let t = DateTime::from_secs(1_790_000_000);
        assert_eq!(time(t, TimestampFormat::Iso8601), "2026-09-21T14:13:20Z");
        assert_eq!(
            time(t, TimestampFormat::Rfc822),
            "Mon, 21 Sep 2026 14:13:20 GMT"
        );
        assert_eq!(time(t, TimestampFormat::UnixTimestamp), "1790000000");
        let frac = DateTime::from_secs_and_nanos(1_790_000_000, 250_000_000);
        assert_eq!(epoch(frac), "1790000000.25");
    }
}
