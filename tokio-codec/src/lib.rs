#![doc(html_root_url = "https://docs.rs/tokio-codec/0.1.1")]
#![deny(missing_docs, missing_debug_implementations, rust_2018_idioms)]
#![cfg_attr(test, deny(warnings))]
#![doc(test(no_crate_inject, attr(deny(rust_2018_idioms))))]

//! Utilities for encoding and decoding frames.
//!
//! Contains adapters to go from streams of bytes, [`AsyncRead`] and
//! [`AsyncWrite`], to framed streams implementing [`Sink`] and [`Stream`].
//! Framed streams are also known as [transports].
//!
//! [`AsyncRead`]: #
//! [`AsyncWrite`]: #
//! [`Sink`]: #
//! [`Stream`]: #
//! [transports]: #

mod bytes_codec;
mod lines_codec;
mod decoder;
mod encoder;
mod framed;

pub use crate::bytes_codec::BytesCodec;
pub use crate::lines_codec::LinesCodec;
pub use crate::decoder::Decoder;
pub use crate::encoder::Encoder;
pub use crate::framed::{Framed, FramedParts};
