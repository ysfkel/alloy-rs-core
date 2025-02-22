use crate::{
    abi::{self, TokenSeq, TokenType},
    Result, Word,
};
use alloc::{borrow::Cow, vec::Vec};

/// An encodable is any type that may be encoded via a given [`SolType`].
///
/// The [`SolType`] trait contains encoding logic for a single associated
/// `RustType`. This trait allows us to plug in encoding logic for other
/// `RustTypes`. Consumers of this library may impl `Encodable<T>` for their
/// types.
///
/// ### Why no `Decodable<T>`?
///
/// We believe in permissive encoders and restrictive decoders. To avoid type
/// ambiguity during the decoding process, we do not allow decoding into
/// arbitrary types. Users desiring this behavior should convert after decoding.
///
/// ### Usage Note
///
/// Rust data may not have a 1:1 mapping to Solidity types. The easiest example
/// of this is [`u64`], which may correspond to any of `uint{40,48,56,64}`.
/// Similarly, [`u128`] covers `uint72-128`. Because of this, usage of this
/// trait is always ambiguous for certain types.
///
/// ```compile_fail,E0284
/// # use alloy_sol_types::{SolType, Encodable, sol_data::*};
/// // Compilation fails due to ambiguity
/// //  error[E0284]: type annotations needed
/// // |
/// // 6 | 100u64.to_tokens();
/// // |        ^^^^^^^^^
/// // |
/// // = note: cannot satisfy `<_ as SolType>::TokenType<'_> == _`
/// // help: try using a fully qualified path to specify the expected types
/// // |
/// // 6 | <u64 as Encodable<T>>::to_tokens(&100u64);
/// // | ++++++++++++++++++++++++++++++++++      ~
/// //
/// 100u64.to_tokens();
/// # Ok::<_, alloy_sol_types::Error>(())
/// ```
///
/// To resolve this, specify the related [`SolType`]. When specifying T it is
/// recommended that you invoke the [`SolType`] methods on `T`, rather than the
/// [`Encodable`] methods.
///
/// ```
/// # use alloy_sol_types::{SolType, Encodable, sol_data::*};
/// # fn main() -> Result<(), alloy_sol_types::Error> {
/// // Not recommended:
/// Encodable::<Uint<64>>::to_tokens(&100u64);
///
/// // Recommended:
/// Uint::<64>::tokenize(&100u64);
/// # Ok(())
/// # }
/// ```
pub trait Encodable<T: ?Sized + SolType> {
    /// Convert the value to tokens.
    fn to_tokens(&self) -> T::TokenType<'_>;

    /// Return the Solidity type name of this value.
    #[inline]
    fn sol_type_name(&self) -> Cow<'static, str> {
        T::sol_type_name()
    }
}

/// A Solidity Type, for ABI encoding and decoding
///
/// This trait is implemented by types that contain ABI encoding and decoding
/// info for Solidity types. Types may be combined to express arbitrarily
/// complex Solidity types.
///
/// ```
/// use alloy_sol_types::{sol_data::*, SolType};
///
/// type DynUint256Array = Array<Uint<256>>;
/// assert_eq!(&DynUint256Array::sol_type_name(), "uint256[]");
///
/// type Erc20FunctionArgs = (Address, Uint<256>);
/// assert_eq!(&Erc20FunctionArgs::sol_type_name(), "(address,uint256)");
///
/// type LargeComplexType = (FixedArray<Array<Bool>, 2>, (FixedBytes<13>, String));
/// assert_eq!(
///     &LargeComplexType::sol_type_name(),
///     "(bool[][2],(bytes13,string))"
/// );
/// ```
///
/// These types are zero cost representations of Solidity types. They do not
/// exist at runtime. They ONLY contain information about the type, they do not
/// carry any data.
///
/// ### Implementer's Guide
///
/// We do not recommend implementing this trait directly. Instead, we recommend
/// using the [`crate::sol`] proc macro to parse a Solidity structdef into a
/// native Rust struct.
///
/// ```
/// alloy_sol_types::sol! {
///     struct MyStruct {
///         bool a;
///         bytes2 b;
///     }
/// }
///
/// // This is the native rust representation of a Solidity type!
/// // How cool is that!
/// const MY_STRUCT: MyStruct = MyStruct {
///     a: true,
///     b: alloy_primitives::FixedBytes([0x01, 0x02]),
/// };
/// ```
pub trait SolType {
    /// The corresponding Rust type.
    type RustType: Encodable<Self> + 'static;

    /// The corresponding ABI token type.
    ///
    /// See implementers of [`TokenType`].
    type TokenType<'a>: TokenType<'a>;

    /// The encoded size of the type, if known at compile time
    const ENCODED_SIZE: Option<usize> = Some(32);

    /// Whether the encoded size is dynamic.
    const DYNAMIC: bool = Self::ENCODED_SIZE.is_none();

    /// The name of the type in Solidity.
    fn sol_type_name() -> Cow<'static, str>;

    /// Calculate the ABI-encoded size of the data, counting both head and tail
    /// words. For a single-word type this will always be 32.
    #[inline]
    fn abi_encoded_size(rust: &Self::RustType) -> usize {
        let _ = rust;
        Self::ENCODED_SIZE.unwrap()
    }

    /// Returns `true` if the given token can be detokenized with this type.
    fn valid_token(token: &Self::TokenType<'_>) -> bool;

    /// Returns an error if the given token cannot be detokenized with this
    /// type.
    #[inline]
    fn type_check(token: &Self::TokenType<'_>) -> Result<()> {
        if Self::valid_token(token) {
            Ok(())
        } else {
            Err(crate::Error::type_check_fail_token(
                token,
                Self::sol_type_name(),
            ))
        }
    }

    /// Detokenize a value from the given token.
    fn detokenize(token: Self::TokenType<'_>) -> Self::RustType;

    /// Tokenizes the given value into this type's token.
    fn tokenize<E: Encodable<Self>>(rust: &E) -> Self::TokenType<'_> {
        rust.to_tokens()
    }

    /// Encode this data according to EIP-712 `encodeData` rules, and hash it
    /// if necessary.
    ///
    /// Implementer's note: All single-word types are encoded as their word.
    /// All multi-word types are encoded as the hash the concatenated data
    /// words for each element
    ///
    /// <https://eips.ethereum.org/EIPS/eip-712#definition-of-encodedata>
    fn eip712_data_word(rust: &Self::RustType) -> Word;

    /// Non-standard Packed Mode ABI encoding.
    ///
    /// See [`abi_encode_packed`][SolType::abi_encode_packed] for more details.
    fn abi_encode_packed_to(rust: &Self::RustType, out: &mut Vec<u8>);

    /// Non-standard Packed Mode ABI encoding.
    ///
    /// This is different from normal ABI encoding:
    /// - types shorter than 32 bytes are concatenated directly, without padding
    ///   or sign extension;
    /// - dynamic types are encoded in-place and without the length;
    /// - array elements are padded, but still encoded in-place.
    ///
    /// More information can be found in the [Solidity docs](https://docs.soliditylang.org/en/latest/abi-spec.html#non-standard-packed-mode).
    #[inline]
    fn abi_encode_packed(rust: &Self::RustType) -> Vec<u8> {
        let mut out = Vec::new();
        Self::abi_encode_packed_to(rust, &mut out);
        out
    }

    /// Encode a single ABI token by wrapping it in a 1-length sequence.
    #[inline]
    fn abi_encode<E: Encodable<Self>>(rust: &E) -> Vec<u8> {
        abi::encode(&rust.to_tokens())
    }

    /// Encode an ABI sequence.
    #[inline]
    fn abi_encode_sequence<E: Encodable<Self>>(rust: &E) -> Vec<u8>
    where
        for<'a> Self::TokenType<'a>: TokenSeq<'a>,
    {
        abi::encode_sequence(&rust.to_tokens())
    }

    /// Encode an ABI sequence suitable for function parameters.
    #[inline]
    fn abi_encode_params<E: Encodable<Self>>(rust: &E) -> Vec<u8>
    where
        for<'a> Self::TokenType<'a>: TokenSeq<'a>,
    {
        abi::encode_params(&rust.to_tokens())
    }

    /// Decode a Rust type from an ABI blob.
    #[inline]
    fn abi_decode(data: &[u8], validate: bool) -> Result<Self::RustType> {
        abi::decode::<Self::TokenType<'_>>(data, validate)
            .and_then(|t| check_decode::<Self>(t, validate))
    }

    /// ABI-decode the given data
    #[inline]
    fn abi_decode_params<'de>(data: &'de [u8], validate: bool) -> Result<Self::RustType>
    where
        Self::TokenType<'de>: TokenSeq<'de>,
    {
        abi::decode_params::<Self::TokenType<'_>>(data, validate)
            .and_then(|t| check_decode::<Self>(t, validate))
    }

    /// ABI-decode a Rust type from an ABI blob.
    #[inline]
    fn abi_decode_sequence<'de>(data: &'de [u8], validate: bool) -> Result<Self::RustType>
    where
        Self::TokenType<'de>: TokenSeq<'de>,
    {
        abi::decode_sequence::<Self::TokenType<'_>>(data, validate)
            .and_then(|t| check_decode::<Self>(t, validate))
    }
}

fn check_decode<T: ?Sized + SolType>(
    token: T::TokenType<'_>,
    validate: bool,
) -> Result<T::RustType> {
    if validate {
        T::type_check(&token)?;
    }
    Ok(T::detokenize(token))
}
