use crate::{Decoder, Encoder};
use bytes::{BufMut, BytesMut};
use futures::io::{AsyncRead, AsyncWrite};
use futures::sink::Sink;
use futures::stream::Stream;
use futures::task::{Context, Poll};
use futures::{ready, try_ready};
use log::trace;
use pin_utils::{unsafe_pinned, unsafe_unpinned};
use std::fmt;
use std::pin::Pin;

const INITIAL_CAPACITY: usize = 8 * 1024;
const BACKPRESSURE_BOUNDARY: usize = INITIAL_CAPACITY;

/// A unified `Stream` and `Sink` interface to an underlying I/O object, using
/// the `Encoder` and `Decoder` traits to encode and decode frames.
pub struct Framed<T, U> {
    inner: T,
    codec: U,
    eof: bool,
    is_readable: bool,
    read_buf: BytesMut,
    write_buf: BytesMut,
}

impl<T: Unpin, U: Unpin> Unpin for Framed<T, U> {}

impl<T, U: Unpin> Framed<T, U> {
    unsafe_pinned!(inner: T);
    unsafe_unpinned!(codec: U);
    unsafe_unpinned!(eof: bool);
    unsafe_unpinned!(is_readable: bool);
    unsafe_unpinned!(read_buf: BytesMut);
    unsafe_unpinned!(write_buf: BytesMut);
}

impl<T, U> Framed<T, U> {
    /// Provides a `Stream` and `Sink` interface for reading and writing to this
    /// `Io` object, using `Decode` and `Encode` to read and write the raw data.
    ///
    /// Raw I/O objects work with byte sequences, but higher-level code usually
    /// wants to batch these into meaningful chunks, called "frames". This
    /// method layers framing on top of an I/O object, by using the `Codec`
    /// traits to handle encoding and decoding of messages frames. Note that
    /// the incoming and outgoing frame types may be distinct.
    ///
    /// This function returns a *single* object that is both `Stream` and
    /// `Sink`; grouping this into a single object is often useful for layering
    /// things like gzip or TLS, which require both read and write access to the
    /// underlying object.
    ///
    /// If you want to work more directly with the streams and sink, consider
    /// calling `split` on the `Framed` returned by this method, which will
    /// break them into separate objects, allowing them to interact more easily.
    pub fn new(inner: T, codec: U) -> Framed<T, U> {
        Framed {
            inner,
            codec,
            eof: false,
            is_readable: false,
            read_buf: BytesMut::with_capacity(INITIAL_CAPACITY),
            write_buf: BytesMut::with_capacity(INITIAL_CAPACITY),
        }
    }

    /// Provides a `Stream` and `Sink` interface for reading and writing to this
    /// `Io` object, using `Decode` and `Encode` to read and write the raw data.
    ///
    /// Raw I/O objects work with byte sequences, but higher-level code usually
    /// wants to batch these into meaningful chunks, called "frames". This
    /// method layers framing on top of an I/O object, by using the `Codec`
    /// traits to handle encoding and decoding of messages frames. Note that
    /// the incoming and outgoing frame types may be distinct.
    ///
    /// This function returns a *single* object that is both `Stream` and
    /// `Sink`; grouping this into a single object is often useful for layering
    /// things like gzip or TLS, which require both read and write access to the
    /// underlying object.
    ///
    /// This objects takes a stream and a readbuffer and a writebuffer. These field
    /// can be obtained from an existing `Framed` with the `into_parts` method.
    ///
    /// If you want to work more directly with the streams and sink, consider
    /// calling `split` on the `Framed` returned by this method, which will
    /// break them into separate objects, allowing them to interact more easily.
    pub fn from_parts(parts: FramedParts<T, U>) -> Framed<T, U> {
        Framed {
            inner: parts.io,
            codec: parts.codec,
            eof: false,
            is_readable: parts.read_buf.len() > 0,
            read_buf: parts.read_buf,
            write_buf: parts.write_buf,
        }
    }

    /// Returns a reference to the underlying I/O stream wrapped by
    /// `Frame`.
    ///
    /// Note that care should be taken to not tamper with the underlying stream
    /// of data coming in as it may corrupt the stream of frames otherwise
    /// being worked with.
    pub fn get_ref(&self) -> &T {
        &self.inner
    }

    /// Returns a mutable reference to the underlying I/O stream wrapped by
    /// `Frame`.
    ///
    /// Note that care should be taken to not tamper with the underlying stream
    /// of data coming in as it may corrupt the stream of frames otherwise
    /// being worked with.
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.inner
    }

    /// Consumes the `Frame`, returning its underlying I/O stream.
    ///
    /// Note that care should be taken to not tamper with the underlying stream
    /// of data coming in as it may corrupt the stream of frames otherwise
    /// being worked with.
    pub fn into_inner(self) -> T {
        self.inner
    }

    /// Consumes the `Frame`, returning its underlying I/O stream, the buffer
    /// with unprocessed data, and the codec.
    ///
    /// Note that care should be taken to not tamper with the underlying stream
    /// of data coming in as it may corrupt the stream of frames otherwise
    /// being worked with.
    pub fn into_parts(self) -> FramedParts<T, U> {
        FramedParts {
            io: self.inner,
            codec: self.codec,
            read_buf: self.read_buf,
            write_buf: self.write_buf,
            _priv: (),
        }
    }
}

impl<T, U> Stream for Framed<T, U>
where
    T: AsyncRead,
    U: Decoder + Unpin,
{
    type Item = Result<U::Item, U::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            // Repeatedly call `decode` or `decode_eof` as long as it is
            // "readable". Readable is defined as not having returned `None`. If
            // the upstream has returned EOF, and the decoder is no longer
            // readable, it can be assumed that the decoder will never become
            // readable again, at which point the stream is terminated.
            if self.is_readable {
                if self.eof {
                    let (codec, buf) = unsafe {
                        let ref_mut = self.as_mut().get_unchecked_mut();
                        (&mut ref_mut.codec, &mut ref_mut.read_buf)
                    };
                    let frame = codec.decode_eof(buf);
                    return Poll::Ready(frame.transpose());
                }

                trace!("attempting to decode a frame");

                let (codec, buf) = unsafe {
                    let ref_mut = self.as_mut().get_unchecked_mut();
                    (&mut ref_mut.codec, &mut ref_mut.read_buf)
                };
                if let Some(frame) = codec.decode(buf).transpose() {
                    trace!("frame decoded from buffer");
                    return Poll::Ready(Some(frame));
                }

                *self.as_mut().is_readable() = false;
            }

            assert!(!self.eof);

            // Otherwise, try to read more data and try again. Make sure we've
            // got room for at least one byte to read to ensure that we don't
            // get a spurious 0 that looks like EOF
            self.as_mut().read_buf().reserve(1);

            let n = unsafe {
                let (inner, buf) = {
                    let ref_mut = self.as_mut().get_unchecked_mut();
                    (
                        Pin::new_unchecked(&mut ref_mut.inner),
                        &mut ref_mut.read_buf,
                    )
                };
                let b = buf.bytes_mut();
                inner.initializer().initialize(b);
                let n = match ready!(inner.poll_read(cx, b)) {
                    Ok(n) => n,
                    Err(err) => return Poll::Ready(Some(Err(err.into()))),
                };
                buf.advance_mut(n);
                n
            };

            if n == 0 {
                *self.as_mut().eof() = true;
            }

            *self.as_mut().is_readable() = true;
        }
    }
}

impl<T, U> Sink<U::Item> for Framed<T, U>
where
    T: AsyncWrite,
    U: Encoder + Unpin,
{
    type SinkError = U::Error;

    fn poll_ready(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::SinkError>> {
        // If the buffer is already over 8KiB, then attempt to flush it. If after flushing it's
        // *still* over 8KiB, then apply backpressure (reject the send).
        if self.write_buf.len() >= BACKPRESSURE_BOUNDARY {
            try_ready!(self.as_mut().poll_flush(cx));

            if self.write_buf.len() >= BACKPRESSURE_BOUNDARY {
                return Poll::Pending;
            }
        }

        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, item: U::Item) -> Result<(), Self::SinkError> {
        let (codec, buf) = unsafe {
            let ref_mut = self.get_unchecked_mut();
            (&mut ref_mut.codec, &mut ref_mut.write_buf)
        };
        codec.encode(item, buf)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::SinkError>> {
        trace!("flushing framed transport");

        while !self.write_buf.is_empty() {
            trace!("writing; remaining={}", self.write_buf.len());

            let (inner, buf) = unsafe {
                let ref_mut = self.as_mut().get_unchecked_mut();
                (Pin::new_unchecked(&mut ref_mut.inner), &ref_mut.write_buf)
            };
            let n = try_ready!(inner.poll_write(cx, &buf));

            if n == 0 {
                return Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "failed to \
                     write frame to transport",
                )
                .into()));
            }

            // TODO: Add a way to `bytes` to do this w/o returning the drained
            // data.
            let _ = self.as_mut().write_buf().split_to(n);
        }

        // Try flushing the underlying IO
        try_ready!(self.as_mut().inner().poll_flush(cx));

        trace!("framed transport flushed");
        return Poll::Ready(Ok(()));
    }

    fn poll_close(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::SinkError>> {
        try_ready!(self.as_mut().poll_flush(cx));
        self.as_mut().inner().poll_close(cx).map_err(Into::into)
    }
}

impl<T, U> fmt::Debug for Framed<T, U>
where
    T: fmt::Debug,
    U: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Framed")
            .field("io", &self.inner)
            .field("codec", &self.codec)
            .finish()
    }
}

/// `FramedParts` contains an export of the data of a Framed transport.
/// It can be used to construct a new `Framed` with a different codec.
/// It contains all current buffers and the inner transport.
#[derive(Debug)]
pub struct FramedParts<T, U> {
    /// The inner transport used to read bytes to and write bytes to
    pub io: T,

    /// The codec
    pub codec: U,

    /// The buffer with read but unprocessed data.
    pub read_buf: BytesMut,

    /// A buffer with unprocessed data which are not written yet.
    pub write_buf: BytesMut,

    /// This private field allows us to add additional fields in the future in a
    /// backwards compatible way.
    _priv: (),
}

impl<T, U> FramedParts<T, U> {
    /// Create a new, default, `FramedParts`
    pub fn new(io: T, codec: U) -> FramedParts<T, U> {
        FramedParts {
            io,
            codec,
            read_buf: BytesMut::new(),
            write_buf: BytesMut::new(),
            _priv: (),
        }
    }
}
