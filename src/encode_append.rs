// Copyright 2019 Parity Technologies
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

use core::{iter::ExactSizeIterator, mem};

use crate::alloc::vec::Vec;
use crate::{Encode, Decode, Error};
use crate::compact::{Compact, CompactLen};
use crate::encode_like::EncodeLike;

/// Trait that allows to append items to an encoded representation without
/// decoding all previous added items.
pub trait EncodeAppend {
	/// The item that will be appended.
	type Item: Encode;

	/// Append all items in `iter` to the given `self_encoded` representation
	/// or if `self_encoded` value is empty, `iter` is encoded to the `Self` representation.
	///
	/// # Example
	///
	/// ```
	///# use parity_scale_codec::EncodeAppend;
	///
	/// // Some encoded data
	/// let data = Vec::new();
	///
	/// let item = 8u32;
	/// let encoded = <Vec<u32> as EncodeAppend>::append_or_new(data, std::iter::once(&item)).expect("Adds new element");
	///
	/// // Add multiple element
	/// <Vec<u32> as EncodeAppend>::append_or_new(encoded, &[700u32, 800u32, 10u32]).expect("Adds new elements");
	/// ```
	fn append_or_new<EncodeLikeItem, I>(
		self_encoded: Vec<u8>,
		iter: I,
	) -> Result<Vec<u8>, Error>
	where
		I: IntoIterator<Item = EncodeLikeItem>,
		EncodeLikeItem: EncodeLike<Self::Item>,
		I::IntoIter: ExactSizeIterator;
}

impl<T: Encode> EncodeAppend for Vec<T> {
	type Item = T;

	fn append_or_new<EncodeLikeItem, I>(
		self_encoded: Vec<u8>,
		iter: I,
	) -> Result<Vec<u8>, Error>
	where
		I: IntoIterator<Item = EncodeLikeItem>,
		EncodeLikeItem: EncodeLike<Self::Item>,
		I::IntoIter: ExactSizeIterator,
	{
		append_or_new_vec_with_any_item(self_encoded, iter)
	}
}

impl<T: Encode> EncodeAppend for crate::alloc::collections::VecDeque<T> {
	type Item = T;

	fn append_or_new<EncodeLikeItem, I>(
		self_encoded: Vec<u8>,
		iter: I,
	) -> Result<Vec<u8>, Error>
	where
		I: IntoIterator<Item = EncodeLikeItem>,
		EncodeLikeItem: EncodeLike<Self::Item>,
		I::IntoIter: ExactSizeIterator,
	{
		append_or_new_vec_with_any_item(self_encoded, iter)
	}
}

fn extract_length_data(data: &[u8], input_len: usize) -> Result<(u32, usize, usize), Error> {
	let len = u32::from(Compact::<u32>::decode(&mut &data[..])?);
	let new_len = len
		.checked_add(input_len as u32)
		.ok_or_else(|| "New vec length greater than `u32::TEST_VALUE()`.")?;

	let encoded_len = Compact::<u32>::compact_len(&len);
	let encoded_new_len = Compact::<u32>::compact_len(&new_len);

	Ok((new_len, encoded_len, encoded_new_len))
}

// Item must have same encoding as encoded value in the encoded vec.
fn append_or_new_vec_with_any_item<Item, I>(
	mut self_encoded: Vec<u8>,
	iter: I,
) -> Result<Vec<u8>, Error>
where
	Item: Encode,
	I: IntoIterator<Item = Item>,
	I::IntoIter: ExactSizeIterator,
{
	let iter = iter.into_iter();
	let input_len = iter.len();

	// No data present, just encode the given input data.
	if self_encoded.is_empty() {
		crate::codec::compact_encode_len_to(&mut self_encoded, iter.len())?;
		iter.for_each(|e| e.encode_to(&mut self_encoded));
		return Ok(self_encoded);
	}

	let (new_len, encoded_len, encoded_new_len) = extract_length_data(&self_encoded, input_len)?;

	let replace_len = |dest: &mut Vec<u8>| {
		Compact(new_len).using_encoded(|e| {
			dest[..encoded_new_len].copy_from_slice(e);
		})
	};

	let append_new_elems = |dest: &mut Vec<u8>| iter.for_each(|a| a.encode_to(dest));

	// If old and new encoded len is equal, we don't need to copy the
	// already encoded data.
	if encoded_len == encoded_new_len {
		replace_len(&mut self_encoded);
		append_new_elems(&mut self_encoded);

		Ok(self_encoded)
	} else {
		let size = encoded_new_len + self_encoded.len() - encoded_len;

		let mut res = Vec::with_capacity(size + input_len * mem::size_of::<Item>());
		unsafe { res.set_len(size); }

		// Insert the new encoded len, copy the already encoded data and
		// add the new element.
		replace_len(&mut res);
		res[encoded_new_len..size].copy_from_slice(&self_encoded[encoded_len..]);
		append_new_elems(&mut res);

		Ok(res)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::{Input, Encode, EncodeLike};
	use std::collections::VecDeque;

	const TEST_VALUE: u32 = {
		#[cfg(not(miri))]
		{ 1_000_000 }
		#[cfg(miri)]
		{ 1_000 }
	};

	#[test]
	fn vec_encode_append_works() {
		let encoded = (0..TEST_VALUE).fold(Vec::new(), |encoded, v| {
			<Vec<u32> as EncodeAppend>::append_or_new(encoded, std::iter::once(&v)).unwrap()
		});

		let decoded = Vec::<u32>::decode(&mut &encoded[..]).unwrap();
		assert_eq!(decoded, (0..TEST_VALUE).collect::<Vec<_>>());
	}

	#[test]
	fn vec_encode_append_multiple_items_works() {
		let encoded = (0..TEST_VALUE).fold(Vec::new(), |encoded, v| {
			<Vec<u32> as EncodeAppend>::append_or_new(encoded, &[v, v, v, v]).unwrap()
		});

		let decoded = Vec::<u32>::decode(&mut &encoded[..]).unwrap();
		let expected = (0..TEST_VALUE).fold(Vec::new(), |mut vec, i| {
			vec.append(&mut vec![i, i, i, i]);
			vec
		});
		assert_eq!(decoded, expected);
	}

	#[test]
	fn vecdeque_encode_append_works() {
		let encoded = (0..TEST_VALUE).fold(Vec::new(), |encoded, v| {
			<VecDeque<u32> as EncodeAppend>::append_or_new(encoded, std::iter::once(&v)).unwrap()
		});

		let decoded = VecDeque::<u32>::decode(&mut &encoded[..]).unwrap();
		assert_eq!(decoded, (0..TEST_VALUE).collect::<Vec<_>>());
	}

	#[test]
	fn vecdeque_encode_append_multiple_items_works() {
		let encoded = (0..TEST_VALUE).fold(Vec::new(), |encoded, v| {
			<VecDeque<u32> as EncodeAppend>::append_or_new(encoded, &[v, v, v, v]).unwrap()
		});

		let decoded = VecDeque::<u32>::decode(&mut &encoded[..]).unwrap();
		let expected = (0..TEST_VALUE).fold(Vec::new(), |mut vec, i| {
			vec.append(&mut vec![i, i, i, i]);
			vec
		});
		assert_eq!(decoded, expected);
	}

	#[test]
	fn append_non_copyable() {
		#[derive(Eq, PartialEq, Debug)]
		struct NoCopy { data: u32 }

		impl EncodeLike for NoCopy {}

		impl Encode for NoCopy {
			fn encode(&self) -> Vec<u8> {
				self.data.encode()
			}
		}

		impl Decode for NoCopy {
			fn decode<I: Input>(input: &mut I) -> Result<Self, Error> {
				u32::decode(input).map(|data| Self { data })
			}

			fn skip<I: Input>(input: &mut I) -> Result<(), Error> {
				u32::skip(input)
			}
		}

		let append = NoCopy { data: 100 };
		let data = Vec::new();
		let encoded = <Vec<NoCopy> as EncodeAppend>::append_or_new(data, std::iter::once(&append)).unwrap();

		let decoded = <Vec<NoCopy>>::decode(&mut &encoded[..]).unwrap();
		assert_eq!(vec![append], decoded);
	}

	#[test]
	fn vec_encode_like_append_works() {
		let encoded = (0..TEST_VALUE).fold(Vec::new(), |encoded, v| {
			<Vec<u32> as EncodeAppend>::append_or_new(encoded, std::iter::once(Box::new(v as u32))).unwrap()
		});

		let decoded = Vec::<u32>::decode(&mut &encoded[..]).unwrap();
		assert_eq!(decoded, (0..TEST_VALUE).collect::<Vec<_>>());
	}
}
