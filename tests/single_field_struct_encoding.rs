#[cfg(not(feature="derive"))]
use parity_scale_codec_derive::{Encode, Decode, CompactAs};
#[cfg(feature="derive")]
use parity_scale_codec::{CompactAs, Decode};
use parity_scale_codec::{Compact, Encode, HasCompact, assert_decode};
use serde_derive::{Serialize, Deserialize};

#[derive(Debug, PartialEq, Encode, Decode)]
struct S {
	x: u32,
}

#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
#[derive(Debug, PartialEq, Eq, Clone, Copy, Encode, Decode, CompactAs)]
struct SSkip {
	#[codec(skip)]
	s1: u32,
	x: u32,
	#[codec(skip)]
	s2: u32,
}

#[derive(Debug, PartialEq, Encode, Decode)]
struct Sc {
	#[codec(compact)]
	x: u32,
}

#[derive(Debug, PartialEq, Encode, Decode)]
struct Sh<T: HasCompact> {
	#[codec(encoded_as = "<T as HasCompact>::Type")]
	x: T,
}

#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
#[derive(Debug, PartialEq, Eq, Clone, Copy, Encode, Decode, CompactAs)]
struct U(u32);

#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
#[derive(Debug, PartialEq, Eq, Clone, Copy, Encode, Decode, CompactAs)]
struct U2 { a: u64 }

#[cfg_attr(feature = "std", derive(Serialize, Deserialize))]
#[derive(Debug, PartialEq, Eq, Clone, Copy, Encode, Decode, CompactAs)]
struct USkip(#[codec(skip)] u32, u32, #[codec(skip)] u32);

#[derive(Debug, PartialEq, Encode, Decode)]
struct Uc(#[codec(compact)] u32);

#[derive(Debug, PartialEq, Clone, Encode, Decode)]
struct Ucas(#[codec(compact)] U);

#[derive(Debug, PartialEq, Clone, Encode, Decode)]
struct USkipcas(#[codec(compact)] USkip);

#[derive(Debug, PartialEq, Clone, Encode, Decode)]
struct SSkipcas(#[codec(compact)] SSkip);

#[derive(Debug, PartialEq, Encode, Decode)]
struct Uh<T: HasCompact>(#[codec(encoded_as = "<T as HasCompact>::Type")] T);

#[test]
fn test_encoding() {
	let x = 3u32;
	let s = S { x };
	let s_skip = SSkip { x, s1: Default::default(), s2: Default::default() };
	let sc = Sc { x };
	let sh = Sh { x };
	let u = U(x);
	let u_skip = USkip(Default::default(), x, Default::default());
	let uc = Uc(x);
	let ucom = Compact(u);
	let ucas = Ucas(u);
	let u_skip_cas = USkipcas(u_skip);
	let s_skip_cas = SSkipcas(s_skip);
	let uh = Uh(x);

	let s_encoded: &[u8] = &[3, 0, 0, 0];
	let s_skip_encoded: &[u8] = &[3, 0, 0, 0];
	let sc_encoded: &[u8] = &[12];
	let sh_encoded: &[u8] = &[12];
	let u_encoded: &[u8] = &[3, 0, 0, 0];
	let u_skip_encoded: &[u8] = &[3, 0, 0, 0];
	let uc_encoded: &[u8] = &[12];
	let ucom_encoded: &[u8] = &[12];
	let ucas_encoded: &[u8] = &[12];
	let u_skip_cas_encoded: &[u8] = &[12];
	let s_skip_cas_encoded: &[u8] = &[12];
	let uh_encoded: &[u8] = &[12];

	assert_eq!(s.encode(), s_encoded);
	assert_eq!(s_skip.encode(), s_skip_encoded);
	assert_eq!(sc.encode(), sc_encoded);
	assert_eq!(sh.encode(), sh_encoded);
	assert_eq!(u.encode(), u_encoded);
	assert_eq!(u_skip.encode(), u_skip_encoded);
	assert_eq!(uc.encode(), uc_encoded);
	assert_eq!(ucom.encode(), ucom_encoded);
	assert_eq!(ucas.encode(), ucas_encoded);
	assert_eq!(u_skip_cas.encode(), u_skip_cas_encoded);
	assert_eq!(s_skip_cas.encode(), s_skip_cas_encoded);
	assert_eq!(uh.encode(), uh_encoded);

	assert_decode::<S>(s_encoded, s);
	assert_decode::<SSkip>(s_skip_encoded, s_skip);
	assert_decode::<Sc>(sc_encoded, sc);
	assert_decode::<Sh<_>>(sh_encoded, sh);
	assert_decode::<U>(u_encoded, u);
	assert_decode::<USkip>(u_skip_encoded, u_skip);
	assert_decode::<Uc>(uc_encoded, uc);
	assert_decode::<Compact::<U>>(ucom_encoded, ucom);
	assert_decode::<Ucas>(ucas_encoded, ucas);
	assert_decode::<USkipcas>(u_skip_cas_encoded, u_skip_cas);
	assert_decode::<SSkipcas>(s_skip_cas_encoded, s_skip_cas);
	assert_decode::<Uh<_>>(uh_encoded, uh);
}
