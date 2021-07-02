// Copyright 2017-2021 Parity Technologies
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

//! # Parity SCALE Codec
//!
//! Rust implementation of the SCALE (Simple Concatenated Aggregate Little-Endian) data format
//! for types used in the Parity Substrate framework.
//!
//! SCALE is a light-weight format which allows encoding (and decoding) which makes it highly
//! suitable for resource-constrained execution environments like blockchain runtimes and low-power,
//! low-memory devices.
//!
//! It is important to note that the encoding context (knowledge of how the types and data
//! structures look) needs to be known separately at both encoding and decoding ends.
//! The encoded data does not include this contextual information.
//!
//! To get a better understanding of how the encoding is done for different types,
//! take a look at the
//! [SCALE Code page at the Substrate Knowledge Base](https://substrate.dev/docs/en/knowledgebase/advanced/codec).
//!
//! ## Implementation
//!
//! The codec is implemented using the following traits:
//!
//! ### Encode
//!
//! The `Encode` trait is used for encoding of data into the SCALE format. The `Encode` trait
//! contains the following functions:

//!
//! * `size_hint(&self) -> usize`: Gets the capacity (in bytes) required for the encoded data.
//! This is to avoid double-allocation of memory needed for the encoding.
//! It can be an estimate and does not need to be an exact number.
//! If the size is not known, even no good maximum, then we can skip this function from the trait
//! implementation. This is required to be a cheap operation, so should not involve iterations etc.
//! * `encode_to<T: Output>(&self, dest: &mut T)`: Encodes the value and appends it to a destination
//!   buffer.
//! * `encode(&self) -> Vec<u8>`: Encodes the type data and returns a slice.
//! * `using_encoded<R, F: FnOnce(&[u8]) -> R>(&self, f: F) -> R`: Encodes the type data and
//!   executes a closure on the encoded value.
//! Returns the result from the executed closure.
//!
//! **Note:** Implementations should override `using_encoded` for value types and `encode_to` for
//! allocating types. `size_hint` should be implemented for all types, wherever possible. Wrapper
//! types should override all methods.
//!
//! ### Decode
//!
//! The `Decode` trait is used for deserialization/decoding of encoded data into the respective
//! types.
//!
//! * `fn decode<I: Input>(value: &mut I) -> Result<Self, Error>`: Tries to decode the value from
//!   SCALE format to the type it is called on.
//! Returns an `Err` if the decoding fails.
//!
//! ### CompactAs
//!
//! The `CompactAs` trait is used for wrapping custom types/structs as compact types, which makes
//! them even more space/memory efficient. The compact encoding is described [here](https://substrate.dev/docs/en/knowledgebase/advanced/codec#compactgeneral-integers).
//!
//! * `encode_as(&self) -> &Self::As`: Encodes the type (self) as a compact type.
//! The type `As` is defined in the same trait and its implementation should be compact encode-able.
//! * `decode_from(_: Self::As) -> Result<Self, Error>`: Decodes the type (self) from a compact
//!   encode-able type.
//!
//! ### HasCompact
//!
//! The `HasCompact` trait, if implemented, tells that the corresponding type is a compact
//! encode-able type.
//!
//! ### EncodeLike
//!
//! The `EncodeLike` trait needs to be implemented for each type manually. When using derive, it is
//! done automatically for you. Basically the trait gives you the opportunity to accept multiple
//! types to a function that all encode to the same representation.
//!
//! ## Usage Examples
//!
//! Following are some examples to demonstrate usage of the codec.
//!
//! ### Simple types
//!
//! ```
//! # // Import macros if derive feature is not used.
//! # #[cfg(not(feature="derive"))]
//! # use parity_scale_codec_derive::{Encode, Decode};
//!
//! use parity_scale_codec::{Encode, Decode};
//!
//! #[derive(Debug, PartialEq, Encode, Decode)]
//! enum EnumType {
//! 	#[codec(index = 15)]
//! 	A,
//! 	B(u32, u64),
//! 	C {
//! 		a: u32,
//! 		b: u64,
//! 	},
//! }
//!
//! let a = EnumType::A;
//! let b = EnumType::B(1, 2);
//! let c = EnumType::C { a: 1, b: 2 };
//!
//! a.using_encoded(|ref slice| {
//!     assert_eq!(slice, &b"\x0f");
//! });
//!
//! b.using_encoded(|ref slice| {
//!     assert_eq!(slice, &b"\x01\x01\0\0\0\x02\0\0\0\0\0\0\0");
//! });
//!
//! c.using_encoded(|ref slice| {
//!     assert_eq!(slice, &b"\x02\x01\0\0\0\x02\0\0\0\0\0\0\0");
//! });
//!
//! let mut da: &[u8] = b"\x0f";
//! assert_eq!(EnumType::decode(&mut da).ok(), Some(a));
//!
//! let mut db: &[u8] = b"\x01\x01\0\0\0\x02\0\0\0\0\0\0\0";
//! assert_eq!(EnumType::decode(&mut db).ok(), Some(b));
//!
//! let mut dc: &[u8] = b"\x02\x01\0\0\0\x02\0\0\0\0\0\0\0";
//! assert_eq!(EnumType::decode(&mut dc).ok(), Some(c));
//!
//! let mut dz: &[u8] = &[0];
//! assert_eq!(EnumType::decode(&mut dz).ok(), None);
//!
//! # fn main() { }
//! ```
//!
//! ### Compact type with HasCompact
//!
//! ```
//! # // Import macros if derive feature is not used.
//! # #[cfg(not(feature="derive"))]
//! # use parity_scale_codec_derive::{Encode, Decode};
//!
//! use parity_scale_codec::{Encode, Decode, Compact, HasCompact};
//!
//! #[derive(Debug, PartialEq, Encode, Decode)]
//! struct Test1CompactHasCompact<T: HasCompact> {
//!     #[codec(compact)]
//!     bar: T,
//! }
//!
//! #[derive(Debug, PartialEq, Encode, Decode)]
//! struct Test1HasCompact<T: HasCompact> {
//!     #[codec(encoded_as = "<T as HasCompact>::Type")]
//!     bar: T,
//! }
//!
//! let test_val: (u64, usize) = (0u64, 1usize);
//!
//! let encoded = Test1HasCompact { bar: test_val.0 }.encode();
//! assert_eq!(encoded.len(), test_val.1);
//! assert_eq!(<Test1CompactHasCompact<u64>>::decode(&mut &encoded[..]).unwrap().bar, test_val.0);
//!
//! # fn main() { }
//! ```
//! ### Type with CompactAs
//!
//! ```rust
//! # // Import macros if derive feature is not used.
//! # #[cfg(not(feature="derive"))]
//! # use parity_scale_codec_derive::{Encode, Decode};
//!
//! use serde_derive::{Serialize, Deserialize};
//! use parity_scale_codec::{Encode, Decode, Compact, HasCompact, CompactAs, Error};
//!
//! #[cfg_attr(feature = "std", derive(Serialize, Deserialize, Debug))]
//! #[derive(PartialEq, Eq, Clone)]
//! struct StructHasCompact(u32);
//!
//! impl CompactAs for StructHasCompact {
//!     type As = u32;
//!
//!     fn encode_as(&self) -> &Self::As {
//!         &12
//!     }
//!
//!     fn decode_from(_: Self::As) -> Result<Self, Error> {
//!         Ok(StructHasCompact(12))
//!     }
//! }
//!
//! impl From<Compact<StructHasCompact>> for StructHasCompact {
//!     fn from(_: Compact<StructHasCompact>) -> Self {
//!         StructHasCompact(12)
//!     }
//! }
//!
//! #[derive(Debug, PartialEq, Encode, Decode)]
//! enum TestGenericHasCompact<T> {
//!     A {
//!         #[codec(compact)] a: T
//!     },
//! }
//!
//! let a = TestGenericHasCompact::A::<StructHasCompact> {
//!     a: StructHasCompact(12325678),
//! };
//!
//! let encoded = a.encode();
//! assert_eq!(encoded.len(), 2);
//!
//! # fn main() { }
//! ```
//!
//! ## Derive attributes
//!
//! The derive implementation supports the following attributes:
//! - `codec(dumb_trait_bound)`: This attribute needs to be placed above the type that one of the
//!   trait should be implemented for. It will make the algorithm that determines the to-add trait
//!   bounds fall back to just use the type parameters of the type. This can be useful for situation
//!   where the algorithm includes private types in the public interface. By using this attribute,
//!   you should not get this error/warning again.
//! - `codec(skip)`: Needs to be placed above a field  or variant and makes it to be skipped while
//!   encoding/decoding.
//! - `codec(compact)`: Needs to be placed above a field and makes the field use compact encoding.
//!   (The type needs to support compact encoding.)
//! - `codec(encoded_as = "OtherType")`: Needs to be placed above a field and makes the field being
//!   encoded by using `OtherType`.
//! - `codec(index = 0)`: Needs to be placed above an enum variant to make the variant use the given
//!   index when encoded. By default the index is determined by counting from `0` beginning wth the
//!   first variant.

#![warn(missing_docs)]
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
#[macro_use]
#[doc(hidden)]
pub extern crate alloc;

#[cfg(feature = "parity-scale-codec-derive")]
#[allow(unused_imports)]
#[macro_use]
extern crate parity_scale_codec_derive;

#[cfg(all(feature = "std", test))]
#[macro_use]
extern crate serde_derive;

#[cfg(feature = "parity-scale-codec-derive")]
pub use parity_scale_codec_derive::*;

#[cfg(feature = "std")]
#[doc(hidden)]
pub mod alloc {
	pub use std::boxed;
	pub use std::vec;
	pub use std::string;
	pub use std::borrow;
	pub use std::collections;
	pub use std::sync;
	pub use std::rc;
}

mod codec;
mod compact;
mod joiner;
mod keyedvec;
#[cfg(feature = "bit-vec")]
mod bit_vec;
#[cfg(feature = "generic-array")]
mod generic_array;
mod decode_all;
mod depth_limit;
mod encode_append;
mod encode_like;
mod error;
#[cfg(feature = "max-encoded-len")]
mod max_encoded_len;

pub use self::error::Error;
pub use self::codec::{
	Input, Output, Decode, Encode, Codec, EncodeAsRef, WrapperTypeEncode, WrapperTypeDecode,
	OptionBool, DecodeLength, FullCodec, FullEncode,
};
#[cfg(feature = "std")]
pub use self::codec::IoReader;
pub use self::compact::{Compact, HasCompact, CompactAs, CompactLen};
pub use self::joiner::Joiner;
pub use self::keyedvec::KeyedVec;
pub use self::decode_all::DecodeAll;
pub use self::depth_limit::DecodeLimit;
pub use self::encode_append::EncodeAppend;
pub use self::encode_like::{EncodeLike, Ref};
#[cfg(feature = "max-encoded-len")]
pub use max_encoded_len::MaxEncodedLen;
/// Derive macro for [`MaxEncodedLen`][max_encoded_len::MaxEncodedLen].
///
/// # Examples
///
/// ```
/// # use parity_scale_codec::{Encode, MaxEncodedLen};
/// #[derive(Encode, MaxEncodedLen)]
/// struct Example;
/// ```
///
/// ```
/// # use parity_scale_codec::{Encode, MaxEncodedLen};
/// #[derive(Encode, MaxEncodedLen)]
/// struct TupleStruct(u8, u32);
///
/// assert_eq!(TupleStruct::max_encoded_len(), u8::max_encoded_len() + u32::max_encoded_len());
/// ```
///
/// ```
/// # use parity_scale_codec::{Encode, MaxEncodedLen};
/// #[derive(Encode, MaxEncodedLen)]
/// enum GenericEnum<T> {
/// 	A,
/// 	B(T),
/// }
///
/// assert_eq!(GenericEnum::<u8>::max_encoded_len(), u8::max_encoded_len() + u8::max_encoded_len());
/// assert_eq!(GenericEnum::<u128>::max_encoded_len(), u8::max_encoded_len() + u128::max_encoded_len());
/// ```
///
/// # Within other macros
///
/// Sometimes the `MaxEncodedLen` trait and macro are used within another macro, and it can't be
/// guaranteed that the `parity_scale_codec` module is available at the call site. In that case, the
/// macro should reexport the `parity_scale_codec` module and specify the path to the reexport:
///
/// ```ignore
/// pub use parity_scale_codec as codec;
///
/// #[derive(Encode, MaxEncodedLen)]
/// #[codec(crate = $crate::codec)]
/// struct Example;
/// ```
#[cfg(all(feature = "derive", feature = "max-encoded-len"))]
pub use parity_scale_codec_derive::MaxEncodedLen;

/// Assert Decode::decode and Decode::skip works
///
/// NOTE: this is not part of public API. Only used for internal tests
#[doc(hidden)]
pub fn assert_decode<T>(mut encoded: &[u8], res: T) where
	T: core::fmt::Debug + Decode + PartialEq,
{
	assert_eq!(Decode::decode(&mut encoded.clone()), Ok(res));
	assert_eq!(T::skip(&mut encoded), Ok(()));
	assert!(encoded.is_empty());
}
