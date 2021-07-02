// Copyright 2017-2020 Parity Technologies
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

//! Derive the Decode::skip implementation for the type.

use proc_macro2::{Span, TokenStream, Ident};
use syn::{
	spanned::Spanned,
	Data, Fields, Field, Error, FieldsNamed, FieldsUnnamed
};

use crate::utils;

/// Implement Decode::skip
///
/// * type_name is name of the type to skip, used for error message
/// * input: the variable name for the type [`Input`] in the call to [`skip`].
pub fn quote(data: &Data, type_name: &Ident, input: &TokenStream) -> TokenStream {
	match *data {
		Data::Struct(ref data) => skip_fields(
			&data.fields,
			input,
		),
		Data::Enum(ref data) => {
			let data_variants = || data.variants.iter().filter(|variant| !utils::should_skip(&variant.attrs));

			if data_variants().count() > 256 {
				return Error::new(
					data.variants.span(),
					"Currently only enums with at most 256 variants are encodable."
				).to_compile_error();
			}

			let recurse = data_variants().enumerate().map(|(i, v)| {
				let index = utils::variant_index(v, i);

				let skip = skip_fields(
					&v.fields,
					input,
				);

				quote_spanned! { v.span() =>
					x if x == #index as u8 => #skip,
				}
			});

			let err_msg = format!("No such variant in enum {}", type_name);
			quote! {
				match #input.read_byte()? {
					#( #recurse )*
					// Actually we don't need to check that value is correct.
					x => Err(#err_msg.into()),
				}
			}
		},
		Data::Union(_) => Error::new(Span::call_site(), "Union types are not supported.").to_compile_error(),
	}
}

// Return an expression that skips a field.
// * input: the variable name for the type [`Input`] in the call to [`skip`].
fn skip_field(field: &Field, input: &TokenStream) -> TokenStream {
	let encoded_as = utils::get_encoded_as_type(field);
	let compact = utils::is_compact(field);
	let skip = utils::should_skip(&field.attrs);

	if encoded_as.is_some() as u8 + compact as u8 + skip as u8 > 1 {
		return Error::new(
			field.span(),
			"`encoded_as`, `compact` and `skip` can only be used one at a time!"
		).to_compile_error();
	}

	if compact {
		let field_type = &field.ty;
		quote_spanned! { field.span() =>
			<
				<#field_type as _parity_scale_codec::HasCompact>::Type as _parity_scale_codec::Decode
			>::skip(#input)
		}
	} else if let Some(encoded_as) = encoded_as {
		quote_spanned! { field.span() =>
			<#encoded_as as _parity_scale_codec::Decode>::skip(#input)
		}
	} else if skip {
		quote_spanned! { field.span() => Ok::<(), _parity_scale_codec::Error>(()) }
	} else {
		let field_ty = &field.ty;
		quote_spanned! { field.span() =>
			<#field_ty as _parity_scale_codec::Decode>::skip(#input)
		}
	}
}

// Return an expression that skips fields.
// * input: the variable name for the type [`Input`] in the call to [`skip`].
fn skip_fields(
	fields: &Fields,
	input: &TokenStream,
) -> TokenStream {
	let span = fields.span();
	match fields {
		Fields::Named(FieldsNamed { named: fields , .. })
			| Fields::Unnamed(FieldsUnnamed { unnamed: fields, .. })
		=> {
			let recurse = fields.iter().map(|f| skip_field(f, input));

			quote_spanned! { span => { #( #recurse?; )* Ok(()) } }
		},
		Fields::Unit => quote_spanned! { span => Ok(()) },
	}
}
