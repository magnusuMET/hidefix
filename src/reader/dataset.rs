use byte_slice_cast::{AsMutByteSlice, AsSliceOf, FromByteSlice, ToMutByteSlice};
use bytes::Bytes;
use futures::{Stream, StreamExt};
use std::pin::Pin;

use crate::filters::byteorder::{Order, ToNative};

pub trait Reader {
    /// Reads raw bytes of slice into destination buffer. Returns bytes read.
    fn read_to(
        &mut self,
        indices: Option<&[u64]>,
        counts: Option<&[u64]>,
        dst: &mut [u8],
    ) -> Result<usize, anyhow::Error>;

    /// Byte-order of dataset.
    fn order(&self) -> Order;

    /// Size of datatype.
    fn dsize(&self) -> usize;

    /// Shape of dataset.
    fn shape(&self) -> &[u64];
}

pub trait ReaderExt: Reader {
    /// Reads values into desitination slice. Returns values read.
    fn values_to<T>(
        &mut self,
        indices: Option<&[u64]>,
        counts: Option<&[u64]>,
        dst: &mut [T],
    ) -> Result<usize, anyhow::Error>
    where
        T: ToMutByteSlice,
        [T]: ToNative,
    {
        let r = self.read_to(indices, counts, dst.as_mut_byte_slice())?;
        dst.to_native(self.order());

        Ok(r)
    }

    /// Reads slice of dataset into `Vec<T>`.
    fn values<T>(
        &mut self,
        indices: Option<&[u64]>,
        counts: Option<&[u64]>,
    ) -> Result<Vec<T>, anyhow::Error>
    where
        T: ToMutByteSlice,
        [T]: ToNative,
    {
        let dsz = self.dsize();
        ensure!(
            dsz % std::mem::size_of::<T>() == 0,
            "size of datatype ({}) not multiple of target {}",
            dsz,
            std::mem::size_of::<T>()
        );

        if dsz != std::mem::size_of::<T>() {
            error!("size of datatype ({}) not same as target {}, alignment may not match and result in unsoundness", dsz, std::mem::size_of::<T>());
        }

        let vsz = counts
            .unwrap_or_else(|| self.shape())
            .iter()
            .product::<u64>() as usize
            * dsz
            / std::mem::size_of::<T>();
        let mut values = Vec::with_capacity(vsz);
        unsafe {
            values.set_len(vsz);
        }
        self.values_to(indices, counts, values.as_mut_slice())?;

        Ok(values)
    }
}

impl<T: ?Sized + Reader> ReaderExt for T {}

pub trait Streamer {
    /// Stream slice of dataset as chunks of `Bytes`.
    fn stream(
        &self,
        indices: Option<&[u64]>,
        counts: Option<&[u64]>,
    ) -> Pin<Box<dyn Stream<Item = Result<Bytes, anyhow::Error>> + Send + 'static>>;

    /// Stream slice of dataset as chunks of `Bytes` serialized as XDR/DAP2-DODS.
    fn stream_xdr(
        &self,
        indices: Option<&[u64]>,
        counts: Option<&[u64]>,
    ) -> Pin<Box<dyn Stream<Item = Result<Bytes, anyhow::Error>> + Send + 'static>>;

    /// Byte-order of dataset.
    fn order(&self) -> Order;

    /// Size of datatype.
    fn dsize(&self) -> usize;
}

pub trait StreamerExt: Streamer {
    /// Stream slice of dataset as `Vec<T>`.
    fn stream_values<T>(
        &self,
        indices: Option<&[u64]>,
        counts: Option<&[u64]>,
    ) -> Pin<Box<dyn Stream<Item = Result<Vec<T>, anyhow::Error>> + Send + 'static>>
    where
        T: Unpin + Send + FromByteSlice + Clone,
        [T]: ToNative,
    {
        let order = self.order();

        Box::pin(self.stream(indices, counts).map(move |b| {
            let b = b?;
            let values = b.as_slice_of::<T>()?;

            // Unfortunately we currently need to make a copy since the byte-slice may be
            // un-aligned to the slice of values.
            let mut values = values.to_vec();

            values.to_native(order);

            Ok(values)
        }))
    }
}

impl<T: ?Sized + Streamer> StreamerExt for T {}
