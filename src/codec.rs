// Copyright 2017-2018 Parity Technologies
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Serialization.

use core::fmt;
use core::{
	convert::TryFrom,
	iter::FromIterator,
	marker::PhantomData,
	mem,
	ops::{Deref, Range, RangeInclusive},
	time::Duration,
};
use core::num::{
	NonZeroI8,
	NonZeroI16,
	NonZeroI32,
	NonZeroI64,
	NonZeroI128,
	NonZeroU8,
	NonZeroU16,
	NonZeroU32,
	NonZeroU64,
	NonZeroU128,
};
use arrayvec::ArrayVec;

use byte_slice_cast::{AsByteSlice, AsMutByteSlice, ToMutByteSlice};

#[cfg(any(feature = "std", feature = "full"))]
use crate::alloc::{
	string::String,
	sync::Arc,
	rc::Rc,
};
use crate::alloc::{
	vec::Vec,
	boxed::Box,
	borrow::{Cow, ToOwned},
	collections::{
		BTreeMap, BTreeSet, VecDeque, LinkedList, BinaryHeap
	}
};
use crate::compact::Compact;
use crate::encode_like::EncodeLike;
use crate::Error;

pub(crate) const MAX_PREALLOCATION: usize = 4 * 1024;
const A_BILLION: u32 = 1_000_000_000;

/// Trait that allows reading of data into a slice.
pub trait Input {
	/// Should return the remaining length of the input data. If no information about the input
	/// length is available, `None` should be returned.
	///
	/// The length is used to constrain the preallocation while decoding. Returning a garbage
	/// length can open the doors for a denial of service attack to your application.
	/// Otherwise, returning `None` can decrease the performance of your application.
	fn remaining_len(&mut self) -> Result<Option<usize>, Error>;

	/// Read the exact number of bytes required to fill the given buffer.
	///
	/// Note that this function is similar to `std::io::Read::read_exact` and not
	/// `std::io::Read::read`.
	fn read(&mut self, into: &mut [u8]) -> Result<(), Error>;

	/// Read a single byte from the input.
	fn read_byte(&mut self) -> Result<u8, Error> {
		let mut buf = [0u8];
		self.read(&mut buf[..])?;
		Ok(buf[0])
	}

	/// Skip the exact number of bytes from the input.
	fn skip(&mut self, len: usize) -> Result<(), Error>;

	/// Descend into nested reference when decoding.
	/// This is called when decoding a new refence-based instance,
	/// such as `Vec` or `Box`. Currently all such types are
	/// allocated on the heap.
	fn descend_ref(&mut self) -> Result<(), Error> {
		Ok(())
	}

	/// Ascend to previous structure level when decoding.
	/// This is called when decoding reference-based type is finished.
	fn ascend_ref(&mut self) {}
}

impl<'a> Input for &'a [u8] {
	fn remaining_len(&mut self) -> Result<Option<usize>, Error> {
		Ok(Some(self.len()))
	}

	fn read(&mut self, into: &mut [u8]) -> Result<(), Error> {
		if into.len() > self.len() {
			return Err("Not enough data to fill buffer".into());
		}
		let len = into.len();
		into.copy_from_slice(&self[..len]);
		*self = &self[len..];
		Ok(())
	}

	fn skip(&mut self, len: usize) -> Result<(), Error> {
		if len > self.len() {
			return Err("Not enough data in input to skip".into());
		}
		*self = &self[len..];
		Ok(())
	}
}

#[cfg(feature = "std")]
impl From<std::io::Error> for Error {
	fn from(err: std::io::Error) -> Self {
		use std::io::ErrorKind::*;
		match err.kind() {
			NotFound => "io error: NotFound".into(),
			PermissionDenied => "io error: PermissionDenied".into(),
			ConnectionRefused => "io error: ConnectionRefused".into(),
			ConnectionReset => "io error: ConnectionReset".into(),
			ConnectionAborted => "io error: ConnectionAborted".into(),
			NotConnected => "io error: NotConnected".into(),
			AddrInUse => "io error: AddrInUse".into(),
			AddrNotAvailable => "io error: AddrNotAvailable".into(),
			BrokenPipe => "io error: BrokenPipe".into(),
			AlreadyExists => "io error: AlreadyExists".into(),
			WouldBlock => "io error: WouldBlock".into(),
			InvalidInput => "io error: InvalidInput".into(),
			InvalidData => "io error: InvalidData".into(),
			TimedOut => "io error: TimedOut".into(),
			WriteZero => "io error: WriteZero".into(),
			Interrupted => "io error: Interrupted".into(),
			Other => "io error: Other".into(),
			UnexpectedEof => "io error: UnexpectedEof".into(),
			_ => "io error: Unknown".into(),
		}
	}
}

/// Wrapper that implements Input for any `Read` type.
///
/// This is not optimized for skipping byte.
#[cfg(feature = "std")]
pub struct IoReader<R: std::io::Read>(pub R);

#[cfg(feature = "std")]
impl<R: std::io::Read> Input for IoReader<R> {
	fn remaining_len(&mut self) -> Result<Option<usize>, Error> {
		Ok(None)
	}

	fn read(&mut self, into: &mut [u8]) -> Result<(), Error> {
		self.0.read_exact(into).map_err(Into::into)
	}

	/// Implementation is not optimized.
	fn skip(&mut self, len: usize) -> Result<(), Error> {
		let mut buffer = vec![0; MAX_PREALLOCATION.min(len)];

		let mut remains = len;
		while remains > MAX_PREALLOCATION {
			self.0.read_exact(&mut buffer[..]).map_err::<Error, _>(Into::into)?;
			remains -= MAX_PREALLOCATION;
		}

		self.0.read_exact(&mut buffer[0..remains]).map_err::<Error, _>(Into::into)?;

		Ok(())
	}
}

/// Trait that allows writing of data.
pub trait Output {
	/// Write to the output.
	fn write(&mut self, bytes: &[u8]);

	/// Write a single byte to the output.
	fn push_byte(&mut self, byte: u8) {
		self.write(&[byte]);
	}
}

#[cfg(not(feature = "std"))]
impl Output for Vec<u8> {
	fn write(&mut self, bytes: &[u8]) {
		self.extend_from_slice(bytes)
	}
}

#[cfg(feature = "std")]
impl<W: std::io::Write> Output for W {
	fn write(&mut self, bytes: &[u8]) {
		(self as &mut dyn std::io::Write).write_all(bytes).expect("Codec outputs are infallible");
	}
}


/// !INTERNAL USE ONLY!
///
/// This enum provides type information to optimize encoding/decoding by doing fake specialization.
#[doc(hidden)]
#[non_exhaustive]
pub enum TypeInfo {
	/// Default value of [`Encode::TYPE_INFO`] to not require implementors to set this value in the trait.
	Unknown,
	U8,
	I8,
	U16,
	I16,
	U32,
	I32,
	U64,
	I64,
	U128,
	I128,
}

/// Trait that allows zero-copy write of value-references to slices in LE format.
///
/// Implementations should override `using_encoded` for value types and `encode_to` and `size_hint` for allocating types.
/// Wrapper types should override all methods.
pub trait Encode {
	// !INTERNAL USE ONLY!
	// This const helps SCALE to optimize the encoding/decoding by doing fake specialization.
	#[doc(hidden)]
	const TYPE_INFO: TypeInfo = TypeInfo::Unknown;

	/// If possible give a hint of expected size of the encoding.
	///
	/// This method is used inside default implementation of `encode`
	/// to avoid re-allocations.
	fn size_hint(&self) -> usize {
		0
	}

	/// Convert self to a slice and append it to the destination.
	fn encode_to<T: Output + ?Sized>(&self, dest: &mut T) {
		self.using_encoded(|buf| dest.write(buf));
	}

	/// Convert self to an owned vector.
	fn encode(&self) -> Vec<u8> {
		let mut r = Vec::with_capacity(self.size_hint());
		self.encode_to(&mut r);
		r
	}

	/// Convert self to a slice and then invoke the given closure with it.
	fn using_encoded<R, F: FnOnce(&[u8]) -> R>(&self, f: F) -> R {
		f(&self.encode())
	}

	/// Calculates the encoded size.
	///
	/// Should be used when the encoded data isn't required.
	///
	/// # Note
	///
	/// This works by using a special [`Output`] that only tracks the size. So, there are no allocations inside the
	/// output. However, this can not prevent allocations that some types are doing inside their own encoding.
	fn encoded_size(&self) -> usize {
		let mut size_tracker = SizeTracker { written: 0 };
		self.encode_to(&mut size_tracker);
		size_tracker.written
	}
}

// Implements `Output` and only keeps track of the number of written bytes
struct SizeTracker {
	written: usize,
}

impl Output for SizeTracker {
	fn write(&mut self, bytes: &[u8]) {
		self.written += bytes.len();
	}

	fn push_byte(&mut self, _byte: u8) {
		self.written += 1;
	}
}

/// Trait that allows the length of a collection to be read, without having
/// to read and decode the entire elements.
pub trait DecodeLength {
	/// Return the number of elements in `self_encoded`.
	fn len(self_encoded: &[u8]) -> Result<usize, Error>;
}

/// Trait that allows zero-copy read of value-references from slices in LE format.
pub trait Decode: Sized {
	// !INTERNAL USE ONLY!
	// This const helps SCALE to optimize the encoding/decoding by doing fake specialization.
	#[doc(hidden)]
	const TYPE_INFO: TypeInfo = TypeInfo::Unknown;

	/// Attempt to deserialise the value from input.
	fn decode<I: Input>(input: &mut I) -> Result<Self, Error>;

	/// Attempt to skip the encoded value from input.
	///
	/// The default implementation of this function is just calling [`Decode::decode`].
	/// When possible, an implementation should provided a specialized implementation.
	fn skip<I: Input>(input: &mut I) -> Result<(), Error> {
		Self::decode(input).map(|_| ())
	}

	/// Returns the fixed encoded size of the type.
	///
	/// If it returns `Some(size)` then all possible values of this
	/// type have the given size (in bytes) when encoded.
	///
	/// NOTE: A type with a fixed encoded size may return `None`.
	fn encoded_fixed_size() -> Option<usize> {
		None
	}
}

/// Trait that allows zero-copy read/write of value-references to/from slices in LE format.
pub trait Codec: Decode + Encode {}
impl<S: Decode + Encode> Codec for S {}

/// Trait that bound `EncodeLike` along with `Encode`. Usefull for generic being used in function
/// with `EncodeLike` parameters.
pub trait FullEncode: Encode + EncodeLike {}
impl<S: Encode + EncodeLike> FullEncode for S {}

/// Trait that bound `EncodeLike` along with `Codec`. Usefull for generic being used in function
/// with `EncodeLike` parameters.
pub trait FullCodec: Decode + FullEncode {}
impl<S: Decode + FullEncode> FullCodec for S {}

/// A marker trait for types that wrap other encodable type.
///
/// Such types should not carry any additional information
/// that would require to be encoded, because the encoding
/// is assumed to be the same as the wrapped type.
///
/// The wrapped type that is referred to is the [`Deref::Target`].
pub trait WrapperTypeEncode: Deref {}

impl<T: ?Sized> WrapperTypeEncode for Box<T> {}
impl<T: ?Sized + Encode> EncodeLike for Box<T> {}
impl<T: Encode> EncodeLike<T> for Box<T> {}
impl<T: Encode> EncodeLike<Box<T>> for T {}

impl<T: ?Sized> WrapperTypeEncode for &T {}
impl<T: ?Sized + Encode> EncodeLike for &T {}
impl<T: Encode> EncodeLike<T> for &T {}
impl<T: Encode> EncodeLike<&T> for T {}
impl<T: Encode> EncodeLike<T> for &&T {}
impl<T: Encode> EncodeLike<&&T> for T {}

impl<T: ?Sized> WrapperTypeEncode for &mut T {}
impl<T: ?Sized + Encode> EncodeLike for &mut T {}
impl<T: Encode> EncodeLike<T> for &mut T {}
impl<T: Encode> EncodeLike<&mut T> for T {}

impl<'a, T: ToOwned + ?Sized> WrapperTypeEncode for Cow<'a, T> {}
impl<'a, T: ToOwned + Encode + ?Sized> EncodeLike for Cow<'a, T> {}
impl<'a, T: ToOwned + Encode> EncodeLike<T> for Cow<'a, T> {}
impl<'a, T: ToOwned + Encode> EncodeLike<Cow<'a, T>> for T {}

#[cfg(any(feature = "std", feature = "full"))]
mod feature_full_wrapper_type_encode {
	use super::*;

	impl<T: ?Sized> WrapperTypeEncode for Arc<T> {}
	impl<T: ?Sized + Encode> EncodeLike for Arc<T> {}
	impl<T: Encode> EncodeLike<T> for Arc<T> {}
	impl<T: Encode> EncodeLike<Arc<T>> for T {}

	impl<T: ?Sized> WrapperTypeEncode for Rc<T> {}
	impl<T: ?Sized + Encode> EncodeLike for Rc<T> {}
	impl<T: Encode> EncodeLike<T> for Rc<T> {}
	impl<T: Encode> EncodeLike<Rc<T>> for T {}

	impl WrapperTypeEncode for String {}
	impl EncodeLike for String {}
	impl EncodeLike<&str> for String {}
	impl EncodeLike<String> for &str {}
}

impl<T, X> Encode for X where
	T: Encode + ?Sized,
	X: WrapperTypeEncode<Target = T>,
{
	fn size_hint(&self) -> usize {
		(&**self).size_hint()
	}

	fn using_encoded<R, F: FnOnce(&[u8]) -> R>(&self, f: F) -> R {
		(&**self).using_encoded(f)
	}

	fn encode(&self) -> Vec<u8> {
		(&**self).encode()
	}

	fn encode_to<W: Output + ?Sized>(&self, dest: &mut W) {
		(&**self).encode_to(dest)
	}
}

/// A marker trait for types that can be created solely from other decodable types.
///
/// The decoding of such type is assumed to be the same as the wrapped type.
pub trait WrapperTypeDecode: Sized {
	/// A wrapped type.
	type Wrapped: Into<Self>;
}
impl<T> WrapperTypeDecode for Box<T> {
	type Wrapped = T;
}
#[cfg(any(feature = "std", feature = "full"))]
impl<T> WrapperTypeDecode for Arc<T> {
	type Wrapped = T;
}
#[cfg(any(feature = "std", feature = "full"))]
impl<T> WrapperTypeDecode for Rc<T> {
	type Wrapped = T;
}

impl<T, X> Decode for X where
	T: Decode + Into<X>,
	X: WrapperTypeDecode<Wrapped=T>,
{
	fn decode<I: Input>(input: &mut I) -> Result<Self, Error> {
		input.descend_ref()?;
		let result = Ok(T::decode(input)?.into());
		input.ascend_ref();
		result
	}

	fn skip<I: Input>(input: &mut I) -> Result<(), Error> {
		T::skip(input)
	}
}

/// A macro that matches on a [`TypeInfo`] and expands a given macro per variant.
///
/// The first parameter to the given macro will be the type of variant (e.g. `u8`, `u32`, etc.) and other parameters
/// given to this macro.
///
/// The last parameter is the code that should be executed for the `Unknown` type info.
macro_rules! with_type_info {
	( $type_info:expr, $macro:ident $( ( $( $params:ident ),* ) )?, { $( $unknown_variant:tt )* }, ) => {
		match $type_info {
			TypeInfo::U8 => { $macro!(u8 $( $( , $params )* )? ) },
			TypeInfo::I8 => { $macro!(i8 $( $( , $params )* )? ) },
			TypeInfo::U16 => { $macro!(u16 $( $( , $params )* )? ) },
			TypeInfo::I16 => { $macro!(i16 $( $( , $params )* )? ) },
			TypeInfo::U32 => { $macro!(u32 $( $( , $params )* )? ) },
			TypeInfo::I32 => { $macro!(i32 $( $( , $params )* )? ) },
			TypeInfo::U64 => { $macro!(u64 $( $( , $params )* )? ) },
			TypeInfo::I64 => { $macro!(i64 $( $( , $params )* )? ) },
			TypeInfo::U128 => { $macro!(u128 $( $( , $params )* )? ) },
			TypeInfo::I128 => { $macro!(i128 $( $( , $params )* )? ) },
			TypeInfo::Unknown => { $( $unknown_variant )* },
		}
	};
}

/// Something that can be encoded as a reference.
pub trait EncodeAsRef<'a, T: 'a> {
	/// The reference type that is used for encoding.
	type RefType: Encode + From<&'a T>;
}

impl<T: Encode, E: Encode> Encode for Result<T, E> {
	fn size_hint(&self) -> usize {
		1 + match *self {
			Ok(ref t) => t.size_hint(),
			Err(ref t) => t.size_hint(),
		}
	}

	fn encode_to<W: Output + ?Sized>(&self, dest: &mut W) {
		match *self {
			Ok(ref t) => {
				dest.push_byte(0);
				t.encode_to(dest);
			}
			Err(ref e) => {
				dest.push_byte(1);
				e.encode_to(dest);
			}
		}
	}
}

impl<T, LikeT, E, LikeE> EncodeLike<Result<LikeT, LikeE>> for Result<T, E>
where
	T: EncodeLike<LikeT>,
	LikeT: Encode,
	E: EncodeLike<LikeE>,
	LikeE: Encode,
{}

impl<T: Decode, E: Decode> Decode for Result<T, E> {
	fn decode<I: Input>(input: &mut I) -> Result<Self, Error> {
		match input.read_byte()
			.map_err(|e| e.chain("Could not result variant byte for `Result`"))?
		{
			0 => Ok(
				Ok(T::decode(input).map_err(|e| e.chain("Could not Decode `Result::Ok(T)`"))?)
			),
			1 => Ok(
				Err(E::decode(input).map_err(|e| e.chain("Could not decode `Result::Error(E)`"))?)
			),
			_ => Err("unexpected first byte decoding Result".into()),
		}
	}

	fn skip<I: Input>(input: &mut I) -> Result<(), Error> {
		match input.read_byte()? {
			0 => T::skip(input),
			1 => E::skip(input),
			_ => Err("unexpected first byte skipping Result".into()),
		}
	}
}

/// Shim type because we can't do a specialised implementation for `Option<bool>` directly.
#[derive(Eq, PartialEq, Clone, Copy)]
pub struct OptionBool(pub Option<bool>);

impl fmt::Debug for OptionBool {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		self.0.fmt(f)
	}
}

impl Encode for OptionBool {
	fn size_hint(&self) -> usize {
		1
	}

	fn using_encoded<R, F: FnOnce(&[u8]) -> R>(&self, f: F) -> R {
		f(&[match *self {
			OptionBool(None) => 0u8,
			OptionBool(Some(true)) => 1u8,
			OptionBool(Some(false)) => 2u8,
		}])
	}
}

impl EncodeLike for OptionBool {}

impl Decode for OptionBool {
	fn decode<I: Input>(input: &mut I) -> Result<Self, Error> {
		match input.read_byte()? {
			0 => Ok(OptionBool(None)),
			1 => Ok(OptionBool(Some(true))),
			2 => Ok(OptionBool(Some(false))),
			_ => Err("unexpected first byte decoding OptionBool".into()),
		}
	}

	fn skip<I: Input>(input: &mut I) -> Result<(), Error> {
		input.skip(1)
	}
}

impl<T: EncodeLike<U>, U: Encode> EncodeLike<Option<U>> for Option<T> {}

impl<T: Encode> Encode for Option<T> {
	fn size_hint(&self) -> usize {
		1 + match *self {
			Some(ref t) => t.size_hint(),
			None => 0,
		}
	}

	fn encode_to<W: Output + ?Sized>(&self, dest: &mut W) {
		match *self {
			Some(ref t) => {
				dest.push_byte(1);
				t.encode_to(dest);
			}
			None => dest.push_byte(0),
		}
	}
}

impl<T: Decode> Decode for Option<T> {
	fn decode<I: Input>(input: &mut I) -> Result<Self, Error> {
		match input.read_byte()
			.map_err(|e| e.chain("Could not decode variant byte for `Option`"))?
		{
			0 => Ok(None),
			1 => Ok(
				Some(T::decode(input).map_err(|e| e.chain("Could not decode `Option::Some(T)`"))?)
			),
			_ => Err("unexpected first byte decoding Option".into()),
		}
	}

	fn skip<I: Input>(input: &mut I) -> Result<(), Error> {
		match input.read_byte()? {
			0 => Ok(()),
			1 => T::skip(input),
			_ => Err("unexpecded first byte skipping Option".into()),
		}
	}
}

macro_rules! impl_for_non_zero {
	( $( $name:ty, $from:ty),* $(,)? ) => {
		$(
			impl Encode for $name {
				fn size_hint(&self) -> usize {
					self.get().size_hint()
				}

				fn encode_to<W: Output + ?Sized>(&self, dest: &mut W) {
					self.get().encode_to(dest)
				}

				fn encode(&self) -> Vec<u8> {
					self.get().encode()
				}

				fn using_encoded<R, F: FnOnce(&[u8]) -> R>(&self, f: F) -> R {
					self.get().using_encoded(f)
				}
			}

			impl Decode for $name {
				fn decode<I: Input>(input: &mut I) -> Result<Self, Error> {
					Self::new(<$from as Decode>::decode(input)?)
						.ok_or_else(|| Error::from("cannot create non-zero number from 0"))
				}

				fn skip<I: Input>(input: &mut I) -> Result<(), Error> {
					<$from as Decode>::skip(input)
				}
			}
		)*
	}
}

/// Encode the slice without prepending the len.
///
/// This is equivalent to encoding all the element one by one, but it is optimized for some types.
pub(crate) fn encode_slice_no_len<T: Encode, W: Output + ?Sized>(slice: &[T], dest: &mut W) {
	macro_rules! encode_to {
		( u8, $slice:ident, $dest:ident ) => {{
			let typed = unsafe { mem::transmute::<&[T], &[u8]>(&$slice[..]) };
			$dest.write(&typed)
		}};
		( i8, $slice:ident, $dest:ident ) => {{
			// `i8` has the same size as `u8`. We can just convert it here and write to the
			// dest buffer directly.
			let typed = unsafe { mem::transmute::<&[T], &[u8]>(&$slice[..]) };
			$dest.write(&typed)
		}};
		( $ty:ty, $slice:ident, $dest:ident ) => {{
			if cfg!(target_endian = "little") {
				let typed = unsafe { mem::transmute::<&[T], &[$ty]>(&$slice[..]) };
				$dest.write(<[$ty] as AsByteSlice<$ty>>::as_byte_slice(typed))
			} else {
				for item in $slice.iter() {
					item.encode_to(dest);
				}
			}
		}};
	}

	with_type_info! {
		<T as Encode>::TYPE_INFO,
		encode_to(slice, dest),
		{
			for item in slice.iter() {
				item.encode_to(dest);
			}
		},
	}
}

/// Decode the slice (without the prepended len).
///
/// This is equivalent to decode all elements one by one, but it is optimized in some
/// situation.
pub(crate) fn decode_vec_with_len<T: Decode, I: Input>(
	input: &mut I,
	len: usize,
) -> Result<Vec<T>, Error> {
	fn decode_unoptimized<I: Input, T: Decode>(
		input: &mut I,
		items_len: usize,
	) -> Result<Vec<T>, Error> {
		let input_capacity = input.remaining_len()?
			.unwrap_or(MAX_PREALLOCATION)
			.checked_div(mem::size_of::<T>())
			.unwrap_or(0);
		let mut r = Vec::with_capacity(input_capacity.min(items_len));
		input.descend_ref()?;
		for _ in 0..items_len {
			r.push(T::decode(input)?);
		}
		input.ascend_ref();
		Ok(r)
	}

	macro_rules! decode {
		( $ty:ty, $input:ident, $len:ident ) => {{
			if cfg!(target_endian = "little") || mem::size_of::<T>() == 1 {
				let vec = read_vec_from_u8s::<_, $ty>($input, $len)?;
				Ok(unsafe { mem::transmute::<Vec<$ty>, Vec<T>>(vec) })
			} else {
				decode_unoptimized($input, $len)
			}
		}};
	}

	with_type_info! {
		<T as Decode>::TYPE_INFO,
		decode(input, len),
		{
			decode_unoptimized(input, len)
		},
	}
}

/// Skip the slice (without the prepended len).
///
/// This is equivalent to skip all elements one by one, but it is optimized in some
/// situation.
pub(crate) fn skip_vec_with_len<T: Decode, I: Input>(
	input: &mut I,
	len: usize,
) -> Result<(), Error> {
	macro_rules! skip {
		( $ty:ty, $input:ident, $len:ident ) => {{
			match $len.checked_mul(mem::size_of::<$ty>()) {
				Some(size) => input.skip(size)?,
				// NOTE: this can be optimized by skipping chunks.
				None => for _ in 0..len {
					T::skip(input)?;
				},
			}
			Ok(())
		}};
	}

	with_type_info! {
		<T as Decode>::TYPE_INFO,
		skip(input, len),
		{
			for _ in 0..len {
				T::skip(input)?;
			}
			Ok(())
		},
	}
}

impl_for_non_zero! {
	NonZeroI8, i8,
	NonZeroI16, i16,
	NonZeroI32, i32,
	NonZeroI64, i64,
	NonZeroI128, i128,
	NonZeroU8, u8,
	NonZeroU16, u16,
	NonZeroU32, u32,
	NonZeroU64, u64,
	NonZeroU128, u128,
}

impl<T: Encode, const N: usize> Encode for [T; N] {
	fn size_hint(&self) -> usize {
		mem::size_of::<T>() * N
	}

	fn encode_to<W: Output + ?Sized>(&self, dest: &mut W) {
		encode_slice_no_len(&self[..], dest)
	}
}

impl<T: Decode, const N: usize> Decode for [T; N] {
	fn decode<I: Input>(input: &mut I) -> Result<Self, Error> {
		let mut array = ArrayVec::new();
		for _ in 0..N {
			array.push(T::decode(input)?);
		}

		match array.into_inner() {
			Ok(a) => Ok(a),
			Err(_) => panic!("We decode `N` elements; qed"),
		}
	}

	fn skip<I: Input>(input: &mut I) -> Result<(), Error> {
		macro_rules! skip_array {
			( $ty:ty, $self:ident, $dest:ident ) => {{
				input.skip(mem::size_of::<Self>())
			}};
		}

		with_type_info! {
			<T as Decode>::TYPE_INFO,
			skip_array(self, dest),
			{
				for _ in 0..N {
					T::skip(input)?;
				}
				Ok(())
			},
		}
	}
}

impl<T: EncodeLike<U>, U: Encode, const N: usize> EncodeLike<[U; N]> for [T; N] {}

impl Encode for str {
	fn size_hint(&self) -> usize {
		self.as_bytes().size_hint()
	}

	fn encode_to<W: Output + ?Sized>(&self, dest: &mut W) {
		self.as_bytes().encode_to(dest)
	}

	fn encode(&self) -> Vec<u8> {
		self.as_bytes().encode()
	}

	fn using_encoded<R, F: FnOnce(&[u8]) -> R>(&self, f: F) -> R {
		self.as_bytes().using_encoded(f)
	}
}

impl<'a, T: ToOwned + ?Sized> Decode for Cow<'a, T>
	where <T as ToOwned>::Owned: Decode,
{
	fn decode<I: Input>(input: &mut I) -> Result<Self, Error> {
		Ok(Cow::Owned(Decode::decode(input)?))
	}

	fn skip<I: Input>(input: &mut I) -> Result<(), Error> {
		T::Owned::skip(input)
	}
}

impl<T> EncodeLike for PhantomData<T> {}

impl<T> Encode for PhantomData<T> {
	fn encode_to<W: Output + ?Sized>(&self, _dest: &mut W) {}
}

impl<T> Decode for PhantomData<T> {
	fn decode<I: Input>(_input: &mut I) -> Result<Self, Error> {
		Ok(PhantomData)
	}

	fn skip<I: Input>(_input: &mut I) -> Result<(), Error> {
		Ok(())
	}
}

#[cfg(any(feature = "std", feature = "full"))]
impl Decode for String {
	fn decode<I: Input>(input: &mut I) -> Result<Self, Error> {
		Self::from_utf8(Vec::decode(input)?).map_err(|_| "Invalid utf8 sequence".into())
	}

	fn skip<I: Input>(input: &mut I) -> Result<(), Error> {
		<Vec<u8>>::skip(input)
	}
}

/// Writes the compact encoding of `len` do `dest`.
pub(crate) fn compact_encode_len_to<W: Output + ?Sized>(dest: &mut W, len: usize) -> Result<(), Error> {
	if len > u32::max_value() as usize {
		return Err("Attempted to serialize a collection with too many elements.".into());
	}

	Compact(len as u32).encode_to(dest);
	Ok(())
}

impl<T: Encode> Encode for [T] {
	fn size_hint(&self) -> usize {
		mem::size_of::<u32>() + mem::size_of::<T>() * self.len()
	}

	fn encode_to<W: Output + ?Sized>(&self, dest: &mut W) {
		compact_encode_len_to(dest, self.len()).expect("Compact encodes length");

		encode_slice_no_len(self, dest)
	}
}

/// Create a `Vec<T>` by casting directly from a buffer of read `u8`s
///
/// The encoding of `T` must be equal to its binary representation, and size of `T` must be less or
/// equal to [`MAX_PREALLOCATION`].
pub(crate) fn read_vec_from_u8s<I, T>(input: &mut I, items_len: usize) -> Result<Vec<T>, Error>
where
	I: Input,
	T: ToMutByteSlice + Default + Clone,
{
	debug_assert!(MAX_PREALLOCATION >= mem::size_of::<T>(), "Invalid precondition");

	let byte_len = items_len.checked_mul(mem::size_of::<T>())
		.ok_or_else(|| "Item is too big and cannot be allocated")?;

	let input_len = input.remaining_len()?;

	// If there is input len and it cannot be pre-allocated then return directly.
	if input_len.map(|l| l < byte_len).unwrap_or(false) {
		return Err("Not enough data to decode vector".into())
	}

	// In both these branches we're going to be creating and resizing a Vec<T>,
	// but casting it to a &mut [u8] for reading.

	// Note: we checked that if input_len is some then it can preallocated.
	let r = if input_len.is_some() || byte_len < MAX_PREALLOCATION {
		// Here we pre-allocate the whole buffer.
		let mut items: Vec<T> = vec![Default::default(); items_len];
		let mut bytes_slice = items.as_mut_byte_slice();
		input.read(&mut bytes_slice)?;

		items
	} else {
		// An allowed number of preallocated item.
		// Note: `MAX_PREALLOCATION` is expected to be more or equal to size of `T`, precondition.
		let max_preallocated_items = MAX_PREALLOCATION / mem::size_of::<T>();

		// Here we pre-allocate only the maximum pre-allocation
		let mut items: Vec<T> = vec![];

		let mut items_remains = items_len;

		while items_remains > 0 {
			let items_len_read = max_preallocated_items.min(items_remains);

			let items_len_filled = items.len();
			let items_new_size = items_len_filled + items_len_read;

			items.reserve_exact(items_len_read);
			unsafe {
				items.set_len(items_new_size);
			}

			let bytes_slice = items.as_mut_byte_slice();
			let bytes_len_filled = items_len_filled * mem::size_of::<T>();
			input.read(&mut bytes_slice[bytes_len_filled..])?;

			items_remains = items_remains.saturating_sub(items_len_read);
		}

		items
	};

	Ok(r)
}

impl<T> WrapperTypeEncode for Vec<T> {}
impl<T: EncodeLike<U>, U: Encode> EncodeLike<Vec<U>> for Vec<T> {}
impl<T: EncodeLike<U>, U: Encode> EncodeLike<&[U]> for Vec<T> {}
impl<T: EncodeLike<U>, U: Encode> EncodeLike<Vec<U>> for &[T] {}

impl<T: Decode> Decode for Vec<T> {
	fn decode<I: Input>(input: &mut I) -> Result<Self, Error> {
		<Compact<u32>>::decode(input).and_then(move |Compact(len)| {
			decode_vec_with_len(input, len as usize)
		})
	}

	fn skip<I: Input>(input: &mut I) -> Result<(), Error> {
		<Compact<u32>>::decode(input).and_then(move |Compact(len)| {
			skip_vec_with_len::<T, _>(input, len as usize)
		})
	}
}

macro_rules! impl_codec_through_iterator {
	($(
		$type:ident
		{ $( $generics:ident $( : $decode_additional:ident )? ),* }
		{ $encoded_type:ty }
		{ $( $type_like_generics:ident ),* }
		{ $( $impl_like_generics:tt )* }
	)*) => {$(
		impl<$( $generics: Encode ),*> Encode for $type<$( $generics, )*> {
			fn size_hint(&self) -> usize {
				mem::size_of::<u32>() $( + mem::size_of::<$generics>() * self.len() )*
			}

			fn encode_to<W: Output + ?Sized>(&self, dest: &mut W) {
				compact_encode_len_to(dest, self.len()).expect("Compact encodes length");

				for i in self.iter() {
					i.encode_to(dest);
				}
			}
		}

		impl<$( $generics: Decode $( + $decode_additional )? ),*> Decode
			for $type<$( $generics, )*>
		{
			fn decode<I: Input>(input: &mut I) -> Result<Self, Error> {
				<Compact<u32>>::decode(input).and_then(move |Compact(len)| {
					input.descend_ref()?;
					let result = Result::from_iter((0..len).map(|_| Decode::decode(input)));
					input.ascend_ref();
					result
				})
			}

			fn skip<I: Input>(input: &mut I) -> Result<(), Error> {
				<Vec<$encoded_type>>::skip(input)
			}
		}

		impl<$( $impl_like_generics )*> EncodeLike<$type<$( $type_like_generics ),*>>
			for $type<$( $generics ),*> {}
		impl<$( $impl_like_generics )*> EncodeLike<&[( $( $type_like_generics, )* )]>
			for $type<$( $generics ),*> {}
		impl<$( $impl_like_generics )*> EncodeLike<$type<$( $type_like_generics ),*>>
			for &[( $( $generics, )* )] {}
	)*}
}

impl_codec_through_iterator! {
	BTreeMap { K: Ord, V } { (K, V) } { LikeK, LikeV }
		{ K: EncodeLike<LikeK>, LikeK: Encode, V: EncodeLike<LikeV>, LikeV: Encode }
	BTreeSet { T: Ord } { T } { LikeT } { T: EncodeLike<LikeT>, LikeT: Encode }
	LinkedList { T } { T } { LikeT } { T: EncodeLike<LikeT>, LikeT: Encode }
	BinaryHeap { T: Ord } { T } { LikeT } { T: EncodeLike<LikeT>, LikeT: Encode }
}

impl<T: Encode> EncodeLike for VecDeque<T> {}
impl<T: EncodeLike<U>, U: Encode> EncodeLike<&[U]> for VecDeque<T> {}
impl<T: EncodeLike<U>, U: Encode> EncodeLike<VecDeque<U>> for &[T] {}
impl<T: EncodeLike<U>, U: Encode> EncodeLike<Vec<U>> for VecDeque<T> {}
impl<T: EncodeLike<U>, U: Encode> EncodeLike<VecDeque<U>> for Vec<T> {}

impl<T: Encode> Encode for VecDeque<T> {
	fn size_hint(&self) -> usize {
		mem::size_of::<u32>() + mem::size_of::<T>() * self.len()
	}

	fn encode_to<W: Output + ?Sized>(&self, dest: &mut W) {
		compact_encode_len_to(dest, self.len()).expect("Compact encodes length");

		macro_rules! encode_to {
			( $ty:ty, $self:ident, $dest:ident ) => {{
				if cfg!(target_endian = "little") || mem::size_of::<T>() == 1 {
					let slices = $self.as_slices();
					let typed = unsafe {
						core::mem::transmute::<(&[T], &[T]), (&[$ty], &[$ty])>(slices)
					};

					$dest.write(<[$ty] as AsByteSlice<$ty>>::as_byte_slice(typed.0));
					$dest.write(<[$ty] as AsByteSlice<$ty>>::as_byte_slice(typed.1));
				} else {
					for item in $self {
						item.encode_to($dest);
					}
				}
			}};
		}

		with_type_info! {
			<T as Encode>::TYPE_INFO,
			encode_to(self, dest),
			{
				for item in self {
					item.encode_to(dest);
				}
			},
		}
	}
}

impl<T: Decode> Decode for VecDeque<T> {
	fn decode<I: Input>(input: &mut I) -> Result<Self, Error> {
		Ok(<Vec<T>>::decode(input)?.into())
	}

	fn skip<I: Input>(input: &mut I) -> Result<(), Error> {
		<Vec<T>>::skip(input)
	}
}

impl EncodeLike for () {}

impl Encode for () {
	fn encode_to<W: Output + ?Sized>(&self, _dest: &mut W) {
	}

	fn using_encoded<R, F: FnOnce(&[u8]) -> R>(&self, f: F) -> R {
		f(&[])
	}

	fn encode(&self) -> Vec<u8> {
		Vec::new()
	}
}

impl Decode for () {
	fn decode<I: Input>(_: &mut I) -> Result<(), Error> {
		Ok(())
	}

	fn skip<I: Input>(_input: &mut I) -> Result<(), Error> {
		Ok(())
	}
}

macro_rules! impl_len {
	( $( $type:ident< $($g:ident),* > ),* ) => { $(
		impl<$($g),*> DecodeLength for $type<$($g),*> {
			fn len(mut self_encoded: &[u8]) -> Result<usize, Error> {
				usize::try_from(u32::from(Compact::<u32>::decode(&mut self_encoded)?))
					.map_err(|_| "Failed convert decoded size into usize.".into())
			}
		}
	)*}
}

// Collection types that support compact decode length.
impl_len!(Vec<T>, BTreeSet<T>, BTreeMap<K, V>, VecDeque<T>, BinaryHeap<T>, LinkedList<T>);

macro_rules! tuple_impl {
	(
		($one:ident, $extra:ident),
	) => {
		impl<$one: Encode> Encode for ($one,) {
			fn size_hint(&self) -> usize {
				self.0.size_hint()
			}

			fn encode_to<T: Output + ?Sized>(&self, dest: &mut T) {
				self.0.encode_to(dest);
			}

			fn encode(&self) -> Vec<u8> {
				self.0.encode()
			}

			fn using_encoded<R, F: FnOnce(&[u8]) -> R>(&self, f: F) -> R {
				self.0.using_encoded(f)
			}
		}

		impl<$one: Decode> Decode for ($one,) {
			fn decode<I: Input>(input: &mut I) -> Result<Self, Error> {
				match $one::decode(input) {
					Err(e) => Err(e),
					Ok($one) => Ok(($one,)),
				}
			}

			fn skip<I: Input>(input: &mut I) -> Result<(), Error> {
				$one::skip(input)
			}
		}

		impl<$one: DecodeLength> DecodeLength for ($one,) {
			fn len(self_encoded: &[u8]) -> Result<usize, Error> {
				$one::len(self_encoded)
			}
		}

		impl<$one: EncodeLike<$extra>, $extra: Encode> crate::EncodeLike<($extra,)> for ($one,) {}
	};
	(($first:ident, $fextra:ident), $( ( $rest:ident, $rextra:ident ), )+) => {
		impl<$first: Encode, $($rest: Encode),+> Encode for ($first, $($rest),+) {
			fn size_hint(&self) -> usize {
				let (
					ref $first,
					$(ref $rest),+
				) = *self;
				$first.size_hint()
				$( + $rest.size_hint() )+
			}

			fn encode_to<T: Output + ?Sized>(&self, dest: &mut T) {
				let (
					ref $first,
					$(ref $rest),+
				) = *self;

				$first.encode_to(dest);
				$($rest.encode_to(dest);)+
			}
		}

		impl<$first: Decode, $($rest: Decode),+> Decode for ($first, $($rest),+) {
			fn decode<INPUT: Input>(input: &mut INPUT) -> Result<Self, super::Error> {
				Ok((
					match $first::decode(input) {
						Ok(x) => x,
						Err(e) => return Err(e),
					},
					$(match $rest::decode(input) {
						Ok(x) => x,
						Err(e) => return Err(e),
					},)+
				))
			}

			fn skip<I: Input>(input: &mut I) -> Result<(), Error> {
				$first::skip(input)?;
				$( $rest::skip(input)?; )+
				Ok(())
			}
		}

		impl<$first: EncodeLike<$fextra>, $fextra: Encode,
			$($rest: EncodeLike<$rextra>, $rextra: Encode),+> crate::EncodeLike<($fextra, $( $rextra ),+)>
			for ($first, $($rest),+) {}

		impl<$first: DecodeLength, $($rest),+> DecodeLength for ($first, $($rest),+) {
			fn len(self_encoded: &[u8]) -> Result<usize, Error> {
				$first::len(self_encoded)
			}
		}

		tuple_impl!( $( ($rest, $rextra), )+ );
	}
}

#[allow(non_snake_case)]
mod inner_tuple_impl {
	use super::*;

	tuple_impl!(
		(A0, A1), (B0, B1), (C0, C1), (D0, D1), (E0, E1), (F0, F1), (G0, G1), (H0, H1), (I0, I1),
		(J0, J1), (K0, K1), (L0, L1), (M0, M1), (N0, N1), (O0, O1), (P0, P1), (Q0, Q1), (R0, R1),
	);
}

macro_rules! impl_endians {
	( $( $t:ty; $ty_info:ident ),* ) => { $(
		impl EncodeLike for $t {}

		impl Encode for $t {
			const TYPE_INFO: TypeInfo = TypeInfo::$ty_info;

			fn size_hint(&self) -> usize {
				mem::size_of::<$t>()
			}

			fn using_encoded<R, F: FnOnce(&[u8]) -> R>(&self, f: F) -> R {
				let buf = self.to_le_bytes();
				f(&buf[..])
			}
		}

		impl Decode for $t {
			const TYPE_INFO: TypeInfo = TypeInfo::$ty_info;

			fn decode<I: Input>(input: &mut I) -> Result<Self, Error> {
				let mut buf = [0u8; mem::size_of::<$t>()];
				input.read(&mut buf)?;
				Ok(<$t>::from_le_bytes(buf))
			}

			fn skip<I: Input>(input: &mut I) -> Result<(), Error> {
				input.skip(mem::size_of::<$t>())
			}
		}
	)* }
}
macro_rules! impl_one_byte {
	( $( $t:ty; $ty_info:ident ),* ) => { $(
		impl EncodeLike for $t {}

		impl Encode for $t {
			const TYPE_INFO: TypeInfo = TypeInfo::$ty_info;

			fn size_hint(&self) -> usize {
				mem::size_of::<$t>()
			}

			fn using_encoded<R, F: FnOnce(&[u8]) -> R>(&self, f: F) -> R {
				f(&[*self as u8][..])
			}
		}

		impl Decode for $t {
			const TYPE_INFO: TypeInfo = TypeInfo::$ty_info;

			fn decode<I: Input>(input: &mut I) -> Result<Self, Error> {
				Ok(input.read_byte()? as $t)
			}

			fn skip<I: Input>(input: &mut I) -> Result<(), Error> {
				input.skip(1)
			}
		}
	)* }
}

impl_endians!(u16; U16, u32; U32, u64; U64, u128; U128, i16; I16, i32; I32, i64; I64, i128; I128);
impl_one_byte!(u8; U8, i8; I8);

impl EncodeLike for bool {}

impl Encode for bool {
	fn size_hint(&self) -> usize {
		mem::size_of::<bool>()
	}

	fn using_encoded<R, F: FnOnce(&[u8]) -> R>(&self, f: F) -> R {
		f(&[*self as u8][..])
	}
}

impl Decode for bool {
	fn decode<I: Input>(input: &mut I) -> Result<Self, Error> {
		let byte = input.read_byte()?;
		match byte {
			0 => Ok(false),
			1 => Ok(true),
			_ => Err("Invalid boolean representation".into())
		}
	}

	fn skip<I: Input>(input: &mut I) -> Result<(), Error> {
		input.skip(1)
	}
}

impl Encode for Duration {
	fn size_hint(&self) -> usize {
		mem::size_of::<u64>() + mem::size_of::<u32>()
	}

	fn encode(&self) -> Vec<u8> {
		let secs = self.as_secs();
		let nanos = self.subsec_nanos();
		(secs, nanos).encode()
	}
}

impl Decode for Duration {
	fn decode<I: Input>(input: &mut I) -> Result<Self, Error> {
		let (secs, nanos) = <(u64, u32)>::decode(input)
			.map_err(|e| e.chain("Could not decode `Duration(u64, u32)`"))?;
		if nanos >= A_BILLION {
			Err("Could not decode `Duration`: Number of nanoseconds should not be higher than 10^9.".into())
		} else {
			Ok(Duration::new(secs, nanos))
		}
	}

	fn skip<I: Input>(input: &mut I) -> Result<(), Error> {
		input.skip(mem::size_of::<u64>() + mem::size_of::<u32>())
	}
}

impl EncodeLike for Duration {}

impl<T> Encode for Range<T>
where
	T: Encode
{
	fn size_hint(&self) -> usize {
		2 * mem::size_of::<T>()
	}

	fn encode(&self) -> Vec<u8> {
		(&self.start, &self.end).encode()
	}
}

impl<T> Decode for Range<T>
where
	T: Decode
{
	fn decode<I: Input>(input: &mut I) -> Result<Self, Error> {
		let (start, end) = <(T, T)>::decode(input)
			.map_err(|e| e.chain("Could not decode `Range<T>`"))?;
		Ok(Range { start, end })
	}
}

impl<T> Encode for RangeInclusive<T>
where
	T: Encode
{
	fn size_hint(&self) -> usize {
		2 * mem::size_of::<T>()
	}

	fn encode(&self) -> Vec<u8> {
		(self.start(), self.end()).encode()
	}
}

impl<T> Decode for RangeInclusive<T>
where
	T: Decode
{
	fn decode<I: Input>(input: &mut I) -> Result<Self, Error> {
		let (start, end) = <(T, T)>::decode(input)
			.map_err(|e| e.chain("Could not decode `RangeInclusive<T>`"))?;
		Ok(RangeInclusive::new(start, end))
	}
}


#[cfg(test)]
mod tests {
	use super::*;
	use crate::assert_decode;
	use std::borrow::Cow;

	#[test]
	fn vec_is_sliceable() {
		let v = b"Hello world".to_vec();
		v.using_encoded(|ref slice|
			assert_eq!(slice, &b"\x2cHello world")
		);
	}

	#[test]
	fn encode_borrowed_tuple() {
		let x = vec![1u8, 2, 3, 4];
		let y = 128i64;

		let encoded = (&x, &y).encode();

		assert_decode(&encoded, (x, y));
	}

	#[test]
	fn cow_works() {
		let x = &[1u32, 2, 3, 4, 5, 6][..];
		let y = Cow::Borrowed(&x);
		assert_eq!(x.encode(), y.encode());

		let z: Cow<'_, [u32]> = Cow::decode(&mut &x.encode()[..]).unwrap();
		assert_eq!(*z, *x);

		let mut skip_input = &x.encode()[..];
		assert_eq!(<Cow<'_, [u32]>>::skip(&mut skip_input), Ok(()));
		assert_eq!(skip_input.len(), 0);

	}

	#[test]
	fn cow_string_works() {
		let x = "Hello world!";
		let y = Cow::Borrowed(&x);
		assert_eq!(x.encode(), y.encode());

		let mut skip_input = &x.encode()[..];
		assert_eq!(<Cow<'_, str>>::skip(&mut skip_input), Ok(()));
		assert_eq!(skip_input.len(), 0);

		let z: Cow<'_, str> = Cow::decode(&mut &x.encode()[..]).unwrap();
		assert_eq!(*z, *x);
	}

	fn hexify(bytes: &[u8]) -> String {
		bytes.iter().map(|ref b| format!("{:02x}", b)).collect::<Vec<String>>().join(" ")
	}

	#[test]
	fn string_encoded_as_expected() {
		let value = String::from("Hello, World!");
		let encoded = value.encode();
		assert_eq!(hexify(&encoded), "34 48 65 6c 6c 6f 2c 20 57 6f 72 6c 64 21");
		assert_decode::<String>(&encoded[..], value);
	}

	#[test]
	fn vec_of_u8_encoded_as_expected() {
		let value = vec![0u8, 1, 1, 2, 3, 5, 8, 13, 21, 34];
		let encoded = value.encode();
		assert_eq!(hexify(&encoded), "28 00 01 01 02 03 05 08 0d 15 22");
		assert_decode::<Vec<u8>>(&encoded[..], value);
	}

	#[test]
	fn vec_of_i16_encoded_as_expected() {
		let value = vec![0i16, 1, -1, 2, -2, 3, -3];
		let encoded = value.encode();
		assert_eq!(hexify(&encoded), "1c 00 00 01 00 ff ff 02 00 fe ff 03 00 fd ff");
		assert_decode::<Vec<i16>>(&encoded[..], value);
	}

	#[test]
	fn vec_of_option_int_encoded_as_expected() {
		let value = vec![Some(1i8), Some(-1), None];
		let encoded = value.encode();
		assert_eq!(hexify(&encoded), "0c 01 01 01 ff 00");
		assert_decode::<Vec<Option<i8>>>(&encoded[..], value);
	}

	#[test]
	fn vec_of_option_bool_encoded_as_expected() {
		let value = vec![OptionBool(Some(true)), OptionBool(Some(false)), OptionBool(None)];
		let encoded = value.encode();
		assert_eq!(hexify(&encoded), "0c 01 02 00");
		assert_decode::<Vec<OptionBool>>(&encoded[..], value);
	}

	fn test_encode_length<T: Encode + Decode + DecodeLength>(thing: &T, len: usize) {
		assert_eq!(<T as DecodeLength>::len(&mut &thing.encode()[..]).unwrap(), len);
	}

	#[test]
	fn len_works_for_decode_collection_types() {
		let vector = vec![10; 10];
		let mut btree_map: BTreeMap<u32, u32> = BTreeMap::new();
		btree_map.insert(1, 1);
		btree_map.insert(2, 2);
		let mut btree_set: BTreeSet<u32> = BTreeSet::new();
		btree_set.insert(1);
		btree_set.insert(2);
		let mut vd = VecDeque::new();
		vd.push_front(1);
		vd.push_front(2);
		let mut bh = BinaryHeap::new();
		bh.push(1);
		bh.push(2);
		let mut ll = LinkedList::new();
		ll.push_back(1);
		ll.push_back(2);
		let t1: (Vec<_>,) = (vector.clone(),);
		let t2: (Vec<_>, u32) = (vector.clone(), 3u32);

		test_encode_length(&vector, 10);
		test_encode_length(&btree_map, 2);
		test_encode_length(&btree_set, 2);
		test_encode_length(&vd, 2);
		test_encode_length(&bh, 2);
		test_encode_length(&ll, 2);
		test_encode_length(&t1, 10);
		test_encode_length(&t2, 10);
	}

	#[test]
	fn vec_of_string_encoded_as_expected() {
		let value = vec![
			"Hamlet".to_owned(),
			"Война и мир".to_owned(),
			"三国演义".to_owned(),
			"أَلْف لَيْلَة وَلَيْلَة‎".to_owned()
		];
		let encoded = value.encode();
		assert_eq!(hexify(&encoded), "10 18 48 61 6d 6c 65 74 50 d0 92 d0 be d0 b9 d0 bd d0 b0 20 d0 \
			b8 20 d0 bc d0 b8 d1 80 30 e4 b8 89 e5 9b bd e6 bc 94 e4 b9 89 bc d8 a3 d9 8e d9 84 d9 92 \
			d9 81 20 d9 84 d9 8e d9 8a d9 92 d9 84 d9 8e d8 a9 20 d9 88 d9 8e d9 84 d9 8e d9 8a d9 92 \
			d9 84 d9 8e d8 a9 e2 80 8e");
		assert_decode::<Vec<String>>(&encoded[..], value);
	}

	#[derive(Debug, PartialEq)]
	struct MyWrapper(Compact<u32>);
	impl Deref for MyWrapper {
		type Target = Compact<u32>;
		fn deref(&self) -> &Self::Target { &self.0 }
	}
	impl WrapperTypeEncode for MyWrapper {}

	impl From<Compact<u32>> for MyWrapper {
		fn from(c: Compact<u32>) -> Self { MyWrapper(c) }
	}
	impl WrapperTypeDecode for MyWrapper {
		type Wrapped = Compact<u32>;
	}

	#[test]
	fn should_work_for_wrapper_types() {
		let result = vec![0b1100];

		assert_eq!(MyWrapper(3u32.into()).encode(), result);
		assert_decode::<MyWrapper>(&result[..], MyWrapper(3_u32.into()));
	}

	#[test]
	fn codec_vec_deque_u8_and_u16() {
		let mut v_u8 = VecDeque::new();
		let mut v_u16 = VecDeque::new();

		for i in 0..50 {
			v_u8.push_front(i as u8);
			v_u16.push_front(i as u16);
		}
		for i in 50..100 {
			v_u8.push_back(i as u8);
			v_u16.push_back(i as u16);
		}

		assert_decode(&v_u8.encode()[..], v_u8);
		assert_decode(&v_u16.encode()[..], v_u16);
	}

	#[test]
	fn codec_iterator() {
		let t1: BTreeSet<u32> = FromIterator::from_iter((0..10).flat_map(|i| 0..i));
		let t2: LinkedList<u32> = FromIterator::from_iter((0..10).flat_map(|i| 0..i));
		let t3: BinaryHeap<u32> = FromIterator::from_iter((0..10).flat_map(|i| 0..i));
		let t4: BTreeMap<u16, u32> = FromIterator::from_iter(
			(0..10)
				.flat_map(|i| 0..i)
				.map(|i| (i as u16, i + 10))
		);
		let t5: BTreeSet<Vec<u8>> = FromIterator::from_iter((0..10).map(|i| Vec::from_iter(0..i)));
		let t6: LinkedList<Vec<u8>> = FromIterator::from_iter((0..10).map(|i| Vec::from_iter(0..i)));
		let t7: BinaryHeap<Vec<u8>> = FromIterator::from_iter((0..10).map(|i| Vec::from_iter(0..i)));
		let t8: BTreeMap<Vec<u8>, u32> = FromIterator::from_iter(
			(0..10)
				.map(|i| Vec::from_iter(0..i))
				.map(|i| (i.clone(), i.len() as u32))
		);

		assert_decode(&t1.encode()[..], t1);
		assert_decode(&t2.encode()[..], t2);

		let mut skip_input = &t3.encode()[..];
		assert_eq!(<BinaryHeap<u32>>::skip(&mut skip_input), Ok(()));
		assert_eq!(skip_input.len(), 0);
		assert_eq!(
			Decode::decode(&mut &t3.encode()[..]).map(BinaryHeap::into_sorted_vec),
			Ok(t3.into_sorted_vec()),
		);

		assert_decode(&t4.encode()[..], t4);
		assert_decode(&t5.encode()[..], t5);
		assert_decode(&t6.encode()[..], t6);

		let mut skip_input = &t7.encode()[..];
		assert_eq!(<BinaryHeap<Vec<u8>>>::skip(&mut skip_input), Ok(()));
		assert_eq!(skip_input.len(), 0);
		assert_eq!(
			Decode::decode(&mut &t7.encode()[..]).map(BinaryHeap::into_sorted_vec),
			Ok(t7.into_sorted_vec()),
		);

		assert_decode(&t8.encode()[..], t8);
	}

	#[test]
	fn io_reader() {
		let mut io_reader = IoReader(std::io::Cursor::new(&[1u8, 2, 3][..]));

		let mut v = [0; 2];
		io_reader.read(&mut v[..]).unwrap();
		assert_eq!(v, [1, 2]);

		assert_eq!(io_reader.read_byte().unwrap(), 3);

		assert_eq!(io_reader.read_byte(), Err("io error: UnexpectedEof".into()));
	}

	#[test]
	fn shared_references_implement_encode() {
		std::sync::Arc::new(10u32).encode();
		std::rc::Rc::new(10u32).encode();
	}

	#[test]
	fn not_limit_input_test() {
		use crate::Input;

		struct NoLimit<'a>(&'a [u8]);

		impl<'a> Input for NoLimit<'a> {
			fn remaining_len(&mut self) -> Result<Option<usize>, Error> {
				Ok(None)
			}

			fn read(&mut self, into: &mut [u8]) -> Result<(), Error> {
				self.0.read(into)
			}

			fn skip(&mut self, len: usize) -> Result<(), Error> {
				self.0.skip(len)
			}
		}

		let len = MAX_PREALLOCATION * 2 + 1;
		let mut i = Compact(len as u32).encode();
		i.resize(i.len() + len, 0);
		assert_eq!(<Vec<u8>>::decode(&mut NoLimit(&i[..])).unwrap(), vec![0u8; len]);

		let i = Compact(len as u32).encode();
		assert_eq!(
			<Vec<u8>>::decode(&mut NoLimit(&i[..])).err().unwrap().to_string(),
			"Not enough data to fill buffer",
		);

		let i = Compact(1000u32).encode();
		assert_eq!(
			<Vec<u8>>::decode(&mut NoLimit(&i[..])).err().unwrap().to_string(),
			"Not enough data to fill buffer",
		);
	}

	#[test]
	fn io_reader_input_skip_works() {
		let mut input = IoReader(std::io::Cursor::new(vec![1u8, 2, 3, 4]));
		assert_eq!(input.skip(2), Ok(()));
		assert_eq!(input.read_byte(), Ok(3));
		assert_eq!(input.skip(2), Err("io error: UnexpectedEof".into()));

		let mut input = IoReader(std::io::Cursor::new(vec![1u8, 2, 3, 4]));
		assert_eq!(input.skip(2), Ok(()));
		assert_eq!(input.skip(2), Ok(()));
		assert_eq!(input.skip(1), Err("io error: UnexpectedEof".into()));

		let mut input = IoReader(std::io::Cursor::new(vec![0; 2 * MAX_PREALLOCATION]));
		assert_eq!(input.skip(MAX_PREALLOCATION), Ok(()));
		assert_eq!(input.0.position(), MAX_PREALLOCATION as u64);
		assert_eq!(input.skip(MAX_PREALLOCATION + 1), Err("io error: UnexpectedEof".into()));

		let mut input = IoReader(std::io::Cursor::new(vec![0; 2 * MAX_PREALLOCATION + 1]));
		assert_eq!(input.skip(2 * MAX_PREALLOCATION + 1), Ok(()));
		assert_eq!(input.skip(0), Ok(()));
		assert_eq!(input.skip(1), Err("io error: UnexpectedEof".into()));
	}

	#[test]
	fn boolean() {
		assert_eq!(true.encode(), vec![1]);
		assert_eq!(false.encode(), vec![0]);
		assert_eq!(bool::decode(&mut &[1][..]).unwrap(), true);
		assert_eq!(bool::decode(&mut &[0][..]).unwrap(), false);

		let mut input = &[255][..];
		assert_eq!(bool::skip(&mut input), Ok(()));
		assert_eq!(input.len(), 0);
	}

	#[test]
	fn some_encode_like() {
		fn t<B: EncodeLike>() {}
		t::<&[u8]>();
		t::<&str>();
	}

	#[test]
	fn vec_deque_encode_like_vec() {
		let data: VecDeque<u32> = vec![1, 2, 3, 4, 5, 6].into();
		let encoded = data.encode();

		let decoded = Vec::<u32>::decode(&mut &encoded[..]).unwrap();
		assert!(decoded.iter().all(|v| data.contains(&v)));
		assert_eq!(data.len(), decoded.len());

		let encoded = decoded.encode();
		assert_decode::<VecDeque<u32>>(&encoded, data);
	}

	#[test]
	fn vec_decode_right_capacity() {
		let data: Vec<u32> = vec![1, 2, 3];
		let mut encoded = data.encode();
		encoded.resize(encoded.len() * 2, 0);
		let decoded = Vec::<u32>::decode(&mut &encoded[..]).unwrap();
		assert_eq!(data, decoded);
		assert_eq!(decoded.capacity(), decoded.len());
		// Check with non-integer type
		let data: Vec<String> = vec!["1".into(), "2".into(), "3".into()];
		let mut encoded = data.encode();
		encoded.resize(65536, 0);
		let decoded = Vec::<String>::decode(&mut &encoded[..]).unwrap();
		assert_eq!(data, decoded);
		assert_eq!(decoded.capacity(), decoded.len());
	}

	#[test]
	fn duration() {
		let num_secs = 13;
		let num_nanos = 37;

		let duration = Duration::new(num_secs, num_nanos);
		let expected = (num_secs, num_nanos as u32).encode();

		assert_eq!(duration.encode(), expected);
		assert_decode::<Duration>(&expected, duration);
	}

	#[test]
	fn malformed_duration_encoding_fails() {
		// This test should fail, as the number of nanoseconds encoded is exactly 10^9.
		let invalid_nanos = A_BILLION;
		let encoded = (0u64, invalid_nanos).encode();
		assert_eq!(
			Duration::decode(&mut &encoded[..]),
			Err("Could not decode `Duration`: Number of nanoseconds should not be higher than 10^9.".into())
		);

		let num_secs = 1u64;
		let num_nanos = 37u32;
		let invalid_nanos = num_secs as u32 * A_BILLION + num_nanos;
		let encoded = (num_secs, invalid_nanos).encode();
		// This test should fail, as the number of nano seconds encoded is bigger than 10^9.
		assert_eq!(
			Duration::decode(&mut &encoded[..]),
			Err("Could not decode `Duration`: Number of nanoseconds should not be higher than 10^9.".into())
		);

		// Now constructing a valid duration and encoding it. Those asserts should not fail.
		let duration = Duration::from_nanos(invalid_nanos as u64);
		let expected = (num_secs, num_nanos).encode();

		assert_eq!(duration.encode(), expected);
		assert_decode::<Duration>(&expected, duration);
	}

	#[test]
	fn u64_max() {
		let num_secs = u64::max_value();
		let num_nanos = 0;
		let duration = Duration::new(num_secs, num_nanos);
		let expected = (num_secs, num_nanos).encode();

		assert_eq!(duration.encode(), expected);
		assert_decode::<Duration>(&expected, duration);
	}

	#[test]
	fn decoding_does_not_overflow() {
		let num_secs = u64::max_value();
		let num_nanos = A_BILLION;

		// `num_nanos`' carry should make `num_secs` overflow if we were to call `Duration::new()`.
		// This test checks that the we do not call `Duration::new()`.
		let encoded = (num_secs, num_nanos).encode();
		assert_eq!(
			Duration::decode(&mut &encoded[..]),
			Err("Could not decode `Duration`: Number of nanoseconds should not be higher than 10^9.".into())
		);
	}

	#[test]
	fn string_invalid_utf8() {
		// `167, 10` is not a valid utf8 sequence, so this should be an error.
		let bytes: &[u8] = &[20, 114, 167, 10, 20, 114];
		assert_eq!(String::decode(&mut &bytes[..]), Err("Invalid utf8 sequence".into()));
	}

	#[test]
	fn empty_array_encode_and_decode() {
		let data: [u32; 0] = [];
		let encoded = data.encode();
		assert!(encoded.is_empty());
		<[u32; 0]>::decode(&mut &encoded[..]).unwrap();
	}

	fn test_encoded_size(val: impl Encode) {
		let length = val.using_encoded(|v| v.len());

		assert_eq!(length, val.encoded_size());
	}

	struct TestStruct {
		data: Vec<u32>,
		other: u8,
		compact: Compact<u128>,
	}

	impl Encode for TestStruct {
		fn encode_to<W: Output + ?Sized>(&self, dest: &mut W) {
			self.data.encode_to(dest);
			self.other.encode_to(dest);
			self.compact.encode_to(dest);
		}
	}

	#[test]
	fn encoded_size_works() {
		test_encoded_size(120u8);
		test_encoded_size(30u16);
		test_encoded_size(1u32);
		test_encoded_size(2343545u64);
		test_encoded_size(34358394245459854u128);
		test_encoded_size(vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10u32]);
		test_encoded_size(Compact(32445u32));
		test_encoded_size(Compact(34353454453545u128));
		test_encoded_size(TestStruct {
			data: vec![1, 2, 4, 5, 6],
			other: 45,
			compact: Compact(123234545),
		});
	}

	#[test]
	fn ranges() {
		let range = Range { start: 1, end: 100 };
		let range_bytes = (1, 100).encode();
		assert_eq!(range.encode(), range_bytes);
		assert_eq!(Range::decode(&mut &range_bytes[..]), Ok(range));

		let range_inclusive = RangeInclusive::new(1, 100);
		let range_inclusive_bytes = (1, 100).encode();
		assert_eq!(range_inclusive.encode(), range_inclusive_bytes);
		assert_eq!(RangeInclusive::decode(&mut &range_inclusive_bytes[..]), Ok(range_inclusive));
	}
}
