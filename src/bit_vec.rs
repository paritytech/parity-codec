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

use core::mem;

use bitvec::{vec::BitVec, store::BitStore, order::BitOrder, slice::BitSlice, boxed::BitBox};
use byte_slice_cast::{AsByteSlice, ToByteSlice, FromByteSlice, Error as FromByteSliceError};

use crate::codec::{Encode, Decode, Input, Output, Error, read_vec_from_u8s};
use crate::compact::Compact;
use crate::EncodeLike;

impl From<FromByteSliceError> for Error {
	fn from(e: FromByteSliceError) -> Error {
		match e {
			FromByteSliceError::AlignmentMismatch {..} =>
				"failed to cast from byte slice: alignment mismatch".into(),
			FromByteSliceError::LengthMismatch {..} =>
				"failed to cast from byte slice: length mismatch".into(),
			FromByteSliceError::CapacityMismatch {..} =>
				"failed to cast from byte slice: capacity mismatch".into(),
		}
	}
}

impl<O: BitOrder, T: BitStore + ToByteSlice> Encode for BitSlice<O, T> {
	fn encode_to<W: Output>(&self, dest: &mut W) {
		self.to_vec().encode_to(dest)
	}
}

fn reverse_endian(vec_u8: &mut Vec<u8>, size_of_t: usize) {
	for i in 0..vec_u8.len() / size_of_t {
		for j in 0..size_of_t / 2 {
			vec_u8.swap(i * size_of_t + j, i * size_of_t + (size_of_t - 1) - j);
		}
	}
}

/// NOTE: encoding when T is usize is not consistent between plateform and must not be used.
impl<O: BitOrder, T: BitStore + ToByteSlice> Encode for BitVec<O, T> {
	fn encode_to<W: Output>(&self, dest: &mut W) {
		let len = self.len();
		assert!(
			len <= u32::max_value() as usize,
			"Attempted to serialize a collection with too many elements.",
		);
		Compact(len as u32).encode_to(dest);

		let mut vec_u8: Vec<u8> = self.as_slice().as_byte_slice().into();

		let size_of_t = mem::size_of::<T>();
		if cfg!(target_endian = "big") && size_of_t != 1 {
			reverse_endian(&mut vec_u8, size_of_t);
		}

		dest.write(&vec_u8);
	}
}

impl<O: BitOrder, T: BitStore + ToByteSlice> EncodeLike for BitVec<O, T> {}

/// NOTE: decoding when T is usize is not consistent between plateform and must not be used.
impl<O: BitOrder, T: BitStore + FromByteSlice> Decode for BitVec<O, T> {
	fn decode<I: Input>(input: &mut I) -> Result<Self, Error> {
		<Compact<u32>>::decode(input).and_then(move |Compact(bits)| {
			let bits = bits as usize;
			let required_bytes = required_bytes::<T>(bits);

			let mut vec_u8 = read_vec_from_u8s::<I, u8>(input, required_bytes)?;

			let size_of_t = mem::size_of::<T>();
			if cfg!(target_endian = "big") && size_of_t != 1 {
				reverse_endian(&mut vec_u8, size_of_t);
			}

			let mut result = Self::from_slice(T::from_byte_slice(&vec_u8)?);
			assert!(bits <= result.len());
			unsafe { result.set_len(bits); }
			Ok(result)
		})
	}
}

impl<O: BitOrder, T: BitStore + ToByteSlice> Encode for BitBox<O, T> {
	fn encode_to<W: Output>(&self, dest: &mut W) {
		self.as_bitslice().encode_to(dest)
	}
}

impl<O: BitOrder, T: BitStore + ToByteSlice> EncodeLike for BitBox<O, T> {}

impl<O: BitOrder, T: BitStore + FromByteSlice> Decode for BitBox<O, T> {
	fn decode<I: Input>(input: &mut I) -> Result<Self, Error> {
		Ok(Self::from_bitslice(BitVec::<O, T>::decode(input)?.as_bitslice()))
	}
}

// Calculates bytes required to store given amount of `bits` as if they were stored in the array of `T`.
fn required_bytes<T>(bits: usize) -> usize {
	let element_bits = mem::size_of::<T>() * 8;
	(bits + element_bits - 1) / element_bits * mem::size_of::<T>()
}

#[cfg(test)]
mod tests {
	use super::*;
	use bitvec::{bitvec, order::Msb0};
	use crate::codec::MAX_PREALLOCATION;

	macro_rules! test_data {
		($inner_type:ident) => (
			[
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
			]
		)
	}

	#[test]
	fn required_bytes_test() {
		assert_eq!(0, required_bytes::<u8>(0));
		assert_eq!(1, required_bytes::<u8>(1));
		assert_eq!(1, required_bytes::<u8>(7));
		assert_eq!(1, required_bytes::<u8>(8));
		assert_eq!(2, required_bytes::<u8>(9));

		assert_eq!(0, required_bytes::<u16>(0));
		assert_eq!(2, required_bytes::<u16>(1));
		assert_eq!(2, required_bytes::<u16>(15));
		assert_eq!(2, required_bytes::<u16>(16));
		assert_eq!(4, required_bytes::<u16>(17));

		assert_eq!(0, required_bytes::<u32>(0));
		assert_eq!(4, required_bytes::<u32>(1));
		assert_eq!(4, required_bytes::<u32>(31));
		assert_eq!(4, required_bytes::<u32>(32));
		assert_eq!(8, required_bytes::<u32>(33));

		assert_eq!(0, required_bytes::<u64>(0));
		assert_eq!(8, required_bytes::<u64>(1));
		assert_eq!(8, required_bytes::<u64>(63));
		assert_eq!(8, required_bytes::<u64>(64));
		assert_eq!(16, required_bytes::<u64>(65));
	}

	#[test]
	fn bitvec_u8() {
		for v in &test_data!(u8) {
			let encoded = v.encode();
			assert_eq!(*v, BitVec::<Msb0, u8>::decode(&mut &encoded[..]).unwrap());
		}
	}

	#[test]
	fn bitvec_u16() {
		for v in &test_data!(u16) {
			let encoded = v.encode();
			assert_eq!(*v, BitVec::<Msb0, u16>::decode(&mut &encoded[..]).unwrap());
		}
	}

	#[test]
	fn bitvec_u32() {
		for v in &test_data!(u32) {
			let encoded = v.encode();
			assert_eq!(*v, BitVec::<Msb0, u32>::decode(&mut &encoded[..]).unwrap());
		}
	}

	#[test]
	fn bitvec_u64() {
		for v in &test_data!(u64) {
			let encoded = dbg!(v.encode());
			assert_eq!(*v, BitVec::<Msb0, u64>::decode(&mut &encoded[..]).unwrap());
		}
	}

	#[test]
	fn bitslice() {
		let data: &[u8] = &[0x69];
		let slice = BitSlice::<Msb0, u8>::from_slice(data);
		let encoded = slice.encode();
		let decoded = BitVec::<Msb0, u8>::decode(&mut &encoded[..]).unwrap();
		assert_eq!(slice, decoded.as_bitslice());
	}

	#[test]
	fn bitbox() {
		let data: &[u8] = &[5, 10];
		let bb = BitBox::<Msb0, u8>::from_slice(data);
		let encoded = bb.encode();
		let decoded = BitBox::<Msb0, u8>::decode(&mut &encoded[..]).unwrap();
		assert_eq!(bb, decoded);
	}

	#[test]
	fn reverse_endian_works() {
		let data = vec![1, 2, 3, 4, 5, 6, 7, 8];

		let mut data_to_u8 = data.clone();
		reverse_endian(&mut data_to_u8, mem::size_of::<u8>());
		assert_eq!(data_to_u8, data);

		let mut data_to_u16 = data.clone();
		reverse_endian(&mut data_to_u16, mem::size_of::<u16>());
		assert_eq!(data_to_u16, vec![2, 1, 4, 3, 6, 5, 8, 7]);

		let mut data_to_u32 = data.clone();
		reverse_endian(&mut data_to_u32, mem::size_of::<u32>());
		assert_eq!(data_to_u32, vec![4, 3, 2, 1, 8, 7, 6, 5]);

		let mut data_to_u64 = data.clone();
		reverse_endian(&mut data_to_u64, mem::size_of::<u64>());
		assert_eq!(data_to_u64, vec![8, 7, 6, 5, 4, 3, 2, 1]);
	}
}
