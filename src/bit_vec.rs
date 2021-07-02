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

//! `BitVec` specific serialization.

use bitvec::{
	vec::BitVec, store::BitStore, order::BitOrder, slice::BitSlice, boxed::BitBox, mem::BitMemory
};
use crate::{
	codec::{
		Decode, Encode, Input, Output,
		decode_vec_with_len, encode_slice_no_len, skip_vec_with_len,
	},
	compact::Compact,
	EncodeLike,
	Error,
};

impl<O: BitOrder, T: BitStore + Encode> Encode for BitSlice<O, T> {
	fn encode_to<W: Output + ?Sized>(&self, dest: &mut W) {
		let len = self.len();
		assert!(
			len <= u32::max_value() as usize,
			"Attempted to serialize a collection with too many elements.",
		);
		Compact(len as u32).encode_to(dest);

		// NOTE: doc of `BitSlice::as_slice`:
		// > The returned slice handle views all elements touched by self
		//
		// Thus we are sure the slice doesn't contain unused elements at the end.
		let slice = self.as_slice();

		encode_slice_no_len(slice, dest)
	}
}

impl<O: BitOrder, T: BitStore + Encode> Encode for BitVec<O, T> {
	fn encode_to<W: Output + ?Sized>(&self, dest: &mut W) {
		self.as_bitslice().encode_to(dest)
	}
}

impl<O: BitOrder, T: BitStore + Encode> EncodeLike for BitVec<O, T> {}

/// Equivalent of `BitStore::MAX_BITS` on 32bit machine.
const ARCH32BIT_BITSLICE_MAX_BITS: usize = 0x1fff_ffff;

impl<O: BitOrder, T: BitStore + Decode> Decode for BitVec<O, T> {
	fn decode<I: Input>(input: &mut I) -> Result<Self, Error> {
		<Compact<u32>>::decode(input).and_then(move |Compact(bits)| {
			// Otherwise it is impossible to store it on 32bit machine.
			if bits as usize > ARCH32BIT_BITSLICE_MAX_BITS {
				return Err("Attempt to decode a bitvec with too many bits".into());
			}
			let required_elements = required_elements::<T>(bits)? as usize;
			let vec = decode_vec_with_len(input, required_elements)?;

			let mut result = Self::try_from_vec(vec)
				.map_err(|_| {
					Error::from("UNEXPECTED ERROR: `bits` is less or equal to
					`ARCH32BIT_BITSLICE_MAX_BITS`; So BitVec must be able to handle the number of
					segment needed for `bits` to be represented; qed")
				})?;

			assert!(bits as usize <= result.len());
			result.truncate(bits as usize);
			Ok(result)
		})
	}

	fn skip<I: Input>(input: &mut I) -> Result<(), Error> {
		<Compact<u32>>::decode(input).and_then(move |Compact(bits)| {
			// Otherwise it is impossible to store it on 32bit machine.
			if bits as usize > ARCH32BIT_BITSLICE_MAX_BITS {
				return Err("Attempt to decode a bitvec with too many bits".into());
			}
			let required_elements = required_elements::<T>(bits)? as usize;
			skip_vec_with_len::<T, _>(input, required_elements)
		})
	}
}

impl<O: BitOrder, T: BitStore + Encode> Encode for BitBox<O, T> {
	fn encode_to<W: Output + ?Sized>(&self, dest: &mut W) {
		self.as_bitslice().encode_to(dest)
	}
}

impl<O: BitOrder, T: BitStore + Encode> EncodeLike for BitBox<O, T> {}

impl<O: BitOrder, T: BitStore + Decode> Decode for BitBox<O, T> {
	fn decode<I: Input>(input: &mut I) -> Result<Self, Error> {
		Ok(Self::from_bitslice(BitVec::<O, T>::decode(input)?.as_bitslice()))
	}

	fn skip<I: Input>(input: &mut I) -> Result<(), Error> {
		BitVec::<O, T>::skip(input)
	}
}

/// Calculates the number of element `T` required to store given amount of `bits` as if they were
/// stored in `BitVec<_, T>`
///
/// Returns an error if the number of bits + number of bits in element overflow u32 capacity.
/// NOTE: this should never happen if `bits` is already checked to be less than
/// `BitStore::MAX_BITS`.
fn required_elements<T: BitStore>(bits: u32) -> Result<u32, Error> {
	let element_bits = T::Mem::BITS as u32;
	let error = Error::from("Attempt to decode bitvec with too many bits");
	Ok((bits.checked_add(element_bits).ok_or_else(|| error)?  - 1) / element_bits)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::assert_decode;
	use bitvec::{bitvec, order::Msb0};
	use crate::codec::MAX_PREALLOCATION;

	macro_rules! test_data {
		($inner_type:ident) => (
			vec![
				BitVec::<Msb0, $inner_type>::new(),
				bitvec![Msb0, $inner_type; 0],
				bitvec![Msb0, $inner_type; 1],
				bitvec![Msb0, $inner_type; 0, 0],
				bitvec![Msb0, $inner_type; 1, 0],
				bitvec![Msb0, $inner_type; 0, 1],
				bitvec![Msb0, $inner_type; 1, 1],
				bitvec![Msb0, $inner_type; 1, 0, 1],
				bitvec![Msb0, $inner_type; 0, 1, 0, 1, 0, 1, 1],
				bitvec![Msb0, $inner_type; 0, 1, 0, 1, 0, 1, 1, 0],
				bitvec![Msb0, $inner_type; 1, 1, 0, 1, 0, 1, 1, 0, 1],
				bitvec![Msb0, $inner_type; 1, 0, 1, 0, 1, 1, 0, 0, 1, 0, 1, 0, 1, 1, 0],
				bitvec![Msb0, $inner_type; 0, 1, 0, 1, 0, 1, 1, 0, 0, 1, 0, 1, 0, 1, 1, 0],
				bitvec![Msb0, $inner_type; 0, 1, 0, 1, 0, 1, 1, 0, 0, 1, 0, 1, 0, 1, 1, 0, 0],
				bitvec![Msb0, $inner_type; 0; 15],
				bitvec![Msb0, $inner_type; 1; 16],
				bitvec![Msb0, $inner_type; 0; 17],
				bitvec![Msb0, $inner_type; 1; 31],
				bitvec![Msb0, $inner_type; 0; 32],
				bitvec![Msb0, $inner_type; 1; 33],
				bitvec![Msb0, $inner_type; 0; 63],
				bitvec![Msb0, $inner_type; 1; 64],
				bitvec![Msb0, $inner_type; 0; 65],
				bitvec![Msb0, $inner_type; 1; MAX_PREALLOCATION * 8 + 1],
				bitvec![Msb0, $inner_type; 0; MAX_PREALLOCATION * 9],
				bitvec![Msb0, $inner_type; 1; MAX_PREALLOCATION * 32 + 1],
				bitvec![Msb0, $inner_type; 0; MAX_PREALLOCATION * 33],
			].into_iter()
		)
	}

	#[test]
	fn required_items_test() {
		assert_eq!(Ok(0), required_elements::<u8>(0));
		assert_eq!(Ok(1), required_elements::<u8>(1));
		assert_eq!(Ok(1), required_elements::<u8>(7));
		assert_eq!(Ok(1), required_elements::<u8>(8));
		assert_eq!(Ok(2), required_elements::<u8>(9));

		assert_eq!(Ok(0), required_elements::<u16>(0));
		assert_eq!(Ok(1), required_elements::<u16>(1));
		assert_eq!(Ok(1), required_elements::<u16>(15));
		assert_eq!(Ok(1), required_elements::<u16>(16));
		assert_eq!(Ok(2), required_elements::<u16>(17));

		assert_eq!(Ok(0), required_elements::<u32>(0));
		assert_eq!(Ok(1), required_elements::<u32>(1));
		assert_eq!(Ok(1), required_elements::<u32>(31));
		assert_eq!(Ok(1), required_elements::<u32>(32));
		assert_eq!(Ok(2), required_elements::<u32>(33));

		assert_eq!(Ok(0), required_elements::<u64>(0));
		assert_eq!(Ok(1), required_elements::<u64>(1));
		assert_eq!(Ok(1), required_elements::<u64>(63));
		assert_eq!(Ok(1), required_elements::<u64>(64));
		assert_eq!(Ok(2), required_elements::<u64>(65));
	}

	#[test]
	fn bitvec_u8() {
		for v in test_data!(u8) {
			let encoded = v.encode();
			assert_decode::<BitVec<Msb0, u8>>(&encoded, v);
		}
	}

	#[test]
	fn bitvec_u16() {
		for v in test_data!(u16) {
			let encoded = v.encode();
			assert_decode::<BitVec<Msb0, u16>>(&encoded, v);
		}
	}

	#[test]
	fn bitvec_u32() {
		for v in test_data!(u32) {
			let encoded = v.encode();
			assert_decode::<BitVec<Msb0, u32>>(&encoded, v);
		}
	}

	#[test]
	fn bitvec_u64() {
		for v in test_data!(u64) {
			let encoded = dbg!(v.encode());
			assert_decode::<BitVec<Msb0, u64>>(&encoded, v);
		}
	}

	#[test]
	fn bitslice() {
		let data: &[u8] = &[0x69];
		let slice = BitSlice::<Msb0, u8>::from_slice(data).unwrap();
		let encoded = slice.encode();
		assert_decode::<BitVec<Msb0, u8>>(&encoded, slice.to_bitvec());
		let decoded = BitVec::<Msb0, u8>::decode(&mut &encoded[..]).unwrap();
		assert_eq!(slice, decoded.as_bitslice());
	}

	#[test]
	fn bitbox() {
		let data: &[u8] = &[5, 10];
		let slice = BitSlice::<Msb0, u8>::from_slice(data).unwrap();
		let bb = BitBox::<Msb0, u8>::from_bitslice(slice);
		let encoded = bb.encode();
		assert_decode::<BitBox<Msb0, u8>>(&encoded, bb);
	}
}
