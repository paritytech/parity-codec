#[cfg(not(feature="derive"))]
use parity_scale_codec_derive::{Encode, Decode};
#[cfg(feature="derive")]
use parity_scale_codec::{Decode};

use parity_scale_codec::{Encode, assert_decode};

#[test]
fn enum_struct_test() {
	#[derive(PartialEq, Debug, Default)]
	struct UncodecType;

	#[derive(PartialEq, Debug)]
	struct UncodecUndefaultType;

	#[derive(PartialEq, Debug, Encode, Decode)]
	enum Enum<T=UncodecType, S=UncodecUndefaultType> {
		#[codec(skip)]
		A(S),
		B {
			#[codec(skip)]
			_b1: T,
			b2: u32,
		},
		C(
			#[codec(skip)]
			T,
			u32,
		),
	}

	#[derive(PartialEq, Debug, Encode, Decode)]
	struct StructNamed<T=UncodecType> {
		#[codec(skip)]
		a: T,
		b: u32,
	}

	#[derive(PartialEq, Debug, Encode, Decode)]
	struct StructUnnamed<T=UncodecType>(
		#[codec(skip)]
		T,
		u32,
	);

	let ea: Enum = Enum::A(UncodecUndefaultType);
	let eb: Enum = Enum::B { _b1: UncodecType, b2: 1 };
	let ec: Enum = Enum::C(UncodecType, 1);
	let sn = StructNamed { a: UncodecType, b: 1 };
	let su = StructUnnamed(UncodecType, 1);

	assert_eq!(ea.encode(), Vec::new());

	let eb_encoded: &[u8] = &eb.encode();
	let ec_encoded: &[u8] = &ec.encode();
	let sn_encoded: &[u8] = &sn.encode();
	let su_encoded: &[u8] = &su.encode();

	assert_decode::<Enum>(eb_encoded, eb);
	assert_decode::<Enum>(ec_encoded, ec);
	assert_decode::<StructNamed>(sn_encoded, sn);
	assert_decode::<StructUnnamed>(su_encoded, su);
}

#[test]
fn skip_enum_struct_inner_variant() {
	// Make sure the skipping does not generates a warning.
	#![deny(warnings)]

	#[derive(Encode, Decode)]
	enum Enum {
		Data {
			some_named: u32,
			#[codec(skip)]
			ignore: Option<u32>,
		}
	}

	let encoded = Enum::Data { some_named: 1, ignore: Some(1) }.encode();
	assert_eq!(vec![0, 1, 0, 0, 0], encoded);
}
