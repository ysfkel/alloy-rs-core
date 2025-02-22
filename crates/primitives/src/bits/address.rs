use crate::{aliases::U160, utils::keccak256, wrap_fixed_bytes, FixedBytes};
use alloc::{
    borrow::Borrow,
    string::{String, ToString},
};
use core::{fmt, str};

/// Error type for address checksum validation.
#[derive(Debug, Copy, Clone)]
pub enum AddressError {
    /// Error while decoding hex.
    Hex(hex::FromHexError),

    /// Invalid ERC-55 checksum.
    InvalidChecksum,
}

impl From<hex::FromHexError> for AddressError {
    #[inline]
    fn from(value: hex::FromHexError) -> Self {
        Self::Hex(value)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for AddressError {
    #[inline]
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Hex(err) => Some(err),
            Self::InvalidChecksum => None,
        }
    }
}

impl fmt::Display for AddressError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hex(err) => err.fmt(f),
            Self::InvalidChecksum => f.write_str("Bad address checksum"),
        }
    }
}

wrap_fixed_bytes!(
    // we implement Display with the checksum, so we don't derive it
    extra_derives: [],
    /// An Ethereum address, 20 bytes in length.
    ///
    /// This type is separate from [`B160`](crate::B160) / [`FixedBytes<20>`]
    /// and is declared with the [`wrap_fixed_bytes!`] macro. This allows us
    /// to implement address-specific functionality.
    ///
    /// The main difference with the generic [`FixedBytes`] implementation is that
    /// [`Display`] formats the address using its [EIP-55] checksum
    /// ([`to_checksum`]).
    /// Use [`Debug`] to display the raw bytes without the checksum.
    ///
    /// [EIP-55]: https://eips.ethereum.org/EIPS/eip-55
    /// [`Debug`]: fmt::Debug
    /// [`Display`]: fmt::Display
    /// [`to_checksum`]: Address::to_checksum
    ///
    /// # Examples
    ///
    /// Parsing and formatting:
    ///
    /// ```
    /// use alloy_primitives::{address, Address};
    ///
    /// let checksummed = "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045";
    /// let expected = address!("d8da6bf26964af9d7eed9e03e53415d37aa96045");
    /// let address = Address::parse_checksummed(checksummed, None).expect("valid checksum");
    /// assert_eq!(address, expected);
    ///
    /// // Format the address with the checksum
    /// assert_eq!(address.to_string(), checksummed);
    /// assert_eq!(address.to_checksum(None), checksummed);
    ///
    /// // Format the compressed checksummed address
    /// assert_eq!(format!("{address:#}"), "0xd8dA…6045");
    ///
    /// // Format the address without the checksum
    /// assert_eq!(format!("{address:?}"), "0xd8da6bf26964af9d7eed9e03e53415d37aa96045");
    /// ```
    pub struct Address<20>;
);

impl From<U160> for Address {
    #[inline]
    fn from(value: U160) -> Self {
        Self(FixedBytes(value.to_be_bytes()))
    }
}

impl From<Address> for U160 {
    #[inline]
    fn from(value: Address) -> Self {
        Self::from_be_bytes(value.0 .0)
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut buf = [0; 42];
        let checksum = self.to_checksum_raw(&mut buf, None);
        if f.alternate() {
            // If the alternate flag is set, use middle-out compression
            // "0x" + first 4 bytes + "…" + last 4 bytes
            f.write_str(&checksum[..6])?;
            f.write_str("…")?;
            f.write_str(&checksum[38..])
        } else {
            f.write_str(checksum)
        }
    }
}

impl Address {
    /// Creates an Ethereum address from an EVM word's upper 20 bytes
    /// (`word[12..]`).
    ///
    /// # Examples
    ///
    /// ```
    /// # use alloy_primitives::{address, b256, Address};
    /// let word = b256!("000000000000000000000000d8da6bf26964af9d7eed9e03e53415d37aa96045");
    /// assert_eq!(
    ///     Address::from_word(word),
    ///     address!("d8da6bf26964af9d7eed9e03e53415d37aa96045")
    /// );
    /// ```
    #[inline]
    #[must_use]
    pub fn from_word(word: FixedBytes<32>) -> Self {
        Self(FixedBytes(word[12..].try_into().unwrap()))
    }

    /// Left-pads the address to 32 bytes (EVM word size).
    ///
    /// # Examples
    ///
    /// ```
    /// # use alloy_primitives::{address, b256, Address};
    /// assert_eq!(
    ///     address!("d8da6bf26964af9d7eed9e03e53415d37aa96045").into_word(),
    ///     b256!("000000000000000000000000d8da6bf26964af9d7eed9e03e53415d37aa96045"),
    /// );
    /// ```
    #[inline]
    #[must_use]
    pub fn into_word(&self) -> FixedBytes<32> {
        let mut word = [0; 32];
        word[12..].copy_from_slice(self.as_slice());
        FixedBytes(word)
    }

    /// Parse an Ethereum address, verifying its [EIP-55] checksum.
    ///
    /// You can optionally specify an [EIP-155 chain ID] to check the address
    /// using [EIP-1191].
    ///
    /// [EIP-55]: https://eips.ethereum.org/EIPS/eip-55
    /// [EIP-155 chain ID]: https://eips.ethereum.org/EIPS/eip-155
    /// [EIP-1191]: https://eips.ethereum.org/EIPS/eip-1191
    ///
    /// # Errors
    ///
    /// This method returns an error if the provided string does not match the
    /// expected checksum.
    ///
    /// # Examples
    ///
    /// ```
    /// # use alloy_primitives::{address, Address};
    /// let checksummed = "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045";
    /// let address = Address::parse_checksummed(checksummed, None).unwrap();
    /// let expected = address!("d8da6bf26964af9d7eed9e03e53415d37aa96045");
    /// assert_eq!(address, expected);
    /// ```
    pub fn parse_checksummed<S: AsRef<str>>(
        s: S,
        chain_id: Option<u64>,
    ) -> Result<Self, AddressError> {
        fn parse_checksummed(s: &str, chain_id: Option<u64>) -> Result<Address, AddressError> {
            // checksummed addresses always start with the "0x" prefix
            if !s.starts_with("0x") {
                return Err(AddressError::Hex(hex::FromHexError::InvalidStringLength))
            }

            let address: Address = s.parse()?;
            if s == address.to_checksum_raw(&mut [0; 42], chain_id) {
                Ok(address)
            } else {
                Err(AddressError::InvalidChecksum)
            }
        }

        parse_checksummed(s.as_ref(), chain_id)
    }

    /// Encodes an Ethereum address to its [EIP-55] checksum.
    ///
    /// You can optionally specify an [EIP-155 chain ID] to encode the address
    /// using [EIP-1191].
    ///
    /// [EIP-55]: https://eips.ethereum.org/EIPS/eip-55
    /// [EIP-155 chain ID]: https://eips.ethereum.org/EIPS/eip-155
    /// [EIP-1191]: https://eips.ethereum.org/EIPS/eip-1191
    ///
    /// # Panics
    ///
    /// This method panics if `buf` is not exactly 42 bytes long.
    ///
    /// # Examples
    ///
    /// ```
    /// # use alloy_primitives::{address, Address};
    /// let address = address!("d8da6bf26964af9d7eed9e03e53415d37aa96045");
    /// let mut buf = [0; 42];
    ///
    /// let checksummed: &str = address.to_checksum_raw(&mut buf, None);
    /// assert_eq!(checksummed, "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045");
    ///
    /// let checksummed: &str = address.to_checksum_raw(&mut buf, Some(1));
    /// assert_eq!(checksummed, "0xD8Da6bf26964Af9d7EEd9e03e53415d37AA96045");
    /// ```
    #[must_use]
    pub fn to_checksum_raw<'a>(&self, buf: &'a mut [u8], chain_id: Option<u64>) -> &'a str {
        assert_eq!(buf.len(), 42, "addr_buf must be 42 bytes long");
        buf[0] = b'0';
        buf[1] = b'x';
        hex::encode_to_slice(self, &mut buf[2..]).unwrap();

        let mut storage;
        let to_hash = match chain_id {
            Some(chain_id) => {
                // A decimal `u64` string is at most 20 bytes long
                storage = [0u8; 2 + 40 + 20];

                // Format the `chain_id` into a stack-allocated buffer using `itoa`
                let mut temp = itoa::Buffer::new();
                let prefix_str = temp.format(chain_id);
                let prefix_len = prefix_str.len();
                debug_assert!(prefix_len <= 20);
                let len = 2 + 40 + prefix_len;

                // SAFETY: prefix_len <= 20; len <= 62; storage.len() == 64
                unsafe {
                    storage
                        .get_unchecked_mut(..prefix_len)
                        .copy_from_slice(prefix_str.as_bytes());
                    storage
                        .get_unchecked_mut(prefix_len..len)
                        .copy_from_slice(buf);
                    storage.get_unchecked(..len)
                }
            }
            None => &buf[2..],
        };
        let hash = keccak256(to_hash);
        let mut hash_hex = [0u8; 64];
        hex::encode_to_slice(hash, &mut hash_hex).unwrap();

        // generates significantly less code than zipping the two arrays, or
        // `.into_iter()`
        for (i, x) in hash_hex.iter().enumerate().take(40) {
            if *x >= b'8' {
                // SAFETY: `addr_buf` is 42 bytes long, `2..42` is always in range
                unsafe { buf.get_unchecked_mut(i + 2).make_ascii_uppercase() };
            }
        }

        // SAFETY: All bytes in the buffer are valid UTF-8
        unsafe { str::from_utf8_unchecked(buf) }
    }

    /// Encodes an Ethereum address to its [EIP-55] checksum.
    ///
    /// You can optionally specify an [EIP-155 chain ID] to encode the address
    /// using [EIP-1191].
    ///
    /// [EIP-55]: https://eips.ethereum.org/EIPS/eip-55
    /// [EIP-155 chain ID]: https://eips.ethereum.org/EIPS/eip-155
    /// [EIP-1191]: https://eips.ethereum.org/EIPS/eip-1191
    ///
    /// # Examples
    ///
    /// ```
    /// # use alloy_primitives::{address, Address};
    /// let address = address!("d8da6bf26964af9d7eed9e03e53415d37aa96045");
    ///
    /// let checksummed: String = address.to_checksum(None);
    /// assert_eq!(checksummed, "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045");
    ///
    /// let checksummed: String = address.to_checksum(Some(1));
    /// assert_eq!(checksummed, "0xD8Da6bf26964Af9d7EEd9e03e53415d37AA96045");
    /// ```
    #[inline]
    #[must_use]
    pub fn to_checksum(&self, chain_id: Option<u64>) -> String {
        self.to_checksum_raw(&mut [0u8; 42], chain_id).to_string()
    }

    /// Computes the `create` address for this address and nonce:
    ///
    /// `keccak256(rlp([sender, nonce]))[12:]`
    ///
    /// # Examples
    ///
    /// ```
    /// # use alloy_primitives::{address, Address};
    /// let sender = address!("b20a608c624Ca5003905aA834De7156C68b2E1d0");
    ///
    /// let expected = address!("00000000219ab540356cBB839Cbe05303d7705Fa");
    /// assert_eq!(sender.create(0), expected);
    ///
    /// let expected = address!("e33c6e89e69d085897f98e92b06ebd541d1daa99");
    /// assert_eq!(sender.create(1), expected);
    /// ```
    #[cfg(feature = "rlp")]
    #[inline]
    #[must_use]
    pub fn create(&self, nonce: u64) -> Self {
        use alloy_rlp::{Encodable, EMPTY_LIST_CODE, EMPTY_STRING_CODE};

        // max u64 encoded length is `1 + u64::BYTES`
        const MAX_LEN: usize = 1 + (1 + 20) + 9;

        let len = 22 + nonce.length();
        debug_assert!(len <= MAX_LEN);

        let mut out = [0u8; MAX_LEN];

        // list header
        // minus 1 to account for the list header itself
        out[0] = EMPTY_LIST_CODE + len as u8 - 1;

        // address header + address
        out[1] = EMPTY_STRING_CODE + 20;
        out[2..22].copy_from_slice(self.as_slice());

        // nonce
        nonce.encode(&mut &mut out[22..]);

        let hash = keccak256(&out[..len]);
        Self::from_word(hash)
    }

    /// Computes the `CREATE2` address of a smart contract as specified in
    /// [EIP-1014]:
    ///
    /// `keccak256(0xff ++ address ++ salt ++ keccak256(init_code))[12:]`
    ///
    /// The `init_code` is the code that, when executed, produces the runtime
    /// bytecode that will be placed into the state, and which typically is used
    /// by high level languages to implement a ‘constructor’.
    ///
    /// [EIP-1014]: https://eips.ethereum.org/EIPS/eip-1014
    ///
    /// # Examples
    ///
    /// ```
    /// # use alloy_primitives::{address, b256, bytes, Address};
    /// let address = address!("8ba1f109551bD432803012645Ac136ddd64DBA72");
    /// let salt = b256!("7c5ea36004851c764c44143b1dcb59679b11c9a68e5f41497f6cf3d480715331");
    /// let init_code = bytes!("6394198df16000526103ff60206004601c335afa6040516060f3");
    /// let expected = address!("533ae9d683B10C02EbDb05471642F85230071FC3");
    /// assert_eq!(address.create2_from_code(salt, init_code), expected);
    /// ```
    #[must_use]
    pub fn create2_from_code<S, C>(&self, salt: S, init_code: C) -> Self
    where
        // not `AsRef` because `[u8; N]` does not implement `AsRef<[u8; N]>`
        S: Borrow<[u8; 32]>,
        C: AsRef<[u8]>,
    {
        self._create2(salt.borrow(), &keccak256(init_code.as_ref()).0)
    }

    /// Computes the `CREATE2` address of a smart contract as specified in
    /// [EIP-1014], taking the pre-computed hash of the init code as input:
    ///
    /// `keccak256(0xff ++ address ++ salt ++ init_code_hash)[12:]`
    ///
    /// The `init_code` is the code that, when executed, produces the runtime
    /// bytecode that will be placed into the state, and which typically is used
    /// by high level languages to implement a ‘constructor’.
    ///
    /// [EIP-1014]: https://eips.ethereum.org/EIPS/eip-1014
    ///
    /// # Examples
    ///
    /// ```
    /// # use alloy_primitives::{address, b256, Address};
    /// let address = address!("5C69bEe701ef814a2B6a3EDD4B1652CB9cc5aA6f");
    /// let salt = b256!("2b2f5776e38002e0c013d0d89828fdb06fee595ea2d5ed4b194e3883e823e350");
    /// let init_code_hash = b256!("96e8ac4277198ff8b6f785478aa9a39f403cb768dd02cbee326c3e7da348845f");
    /// let expected = address!("0d4a11d5EEaaC28EC3F61d100daF4d40471f1852");
    /// assert_eq!(address.create2(salt, init_code_hash), expected);
    /// ```
    #[must_use]
    pub fn create2<S, H>(&self, salt: S, init_code_hash: H) -> Self
    where
        // not `AsRef` because `[u8; N]` does not implement `AsRef<[u8; N]>`
        S: Borrow<[u8; 32]>,
        H: Borrow<[u8; 32]>,
    {
        self._create2(salt.borrow(), init_code_hash.borrow())
    }

    // non-generic inner function
    fn _create2(&self, salt: &[u8; 32], init_code_hash: &[u8; 32]) -> Self {
        // note: creating a temporary buffer and copying everything over performs
        // much better than calling `Keccak::update` multiple times
        let mut bytes = [0; 85];
        bytes[0] = 0xff;
        bytes[1..21].copy_from_slice(self.as_slice());
        bytes[21..53].copy_from_slice(salt);
        bytes[53..85].copy_from_slice(init_code_hash);
        let hash = keccak256(bytes);
        Self::from_word(hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hex;

    #[test]
    fn parse() {
        let expected = hex!("0102030405060708090a0b0c0d0e0f1011121314");
        assert_eq!(
            "0102030405060708090a0b0c0d0e0f1011121314"
                .parse::<Address>()
                .unwrap()
                .into_array(),
            expected
        );
        assert_eq!(
            "0x0102030405060708090a0b0c0d0e0f1011121314"
                .parse::<Address>()
                .unwrap(),
            expected
        );
    }

    // https://eips.ethereum.org/EIPS/eip-55
    #[test]
    fn checksum() {
        let addresses = [
            // All caps
            "0x52908400098527886E0F7030069857D2E4169EE7",
            "0x8617E340B3D01FA5F11F306F4090FD50E238070D",
            // All Lower
            "0xde709f2102306220921060314715629080e2fb77",
            "0x27b1fdb04752bbc536007a920d24acb045561c26",
            // Normal
            "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed",
            "0xfB6916095ca1df60bB79Ce92cE3Ea74c37c5d359",
            "0xdbF03B407c01E7cD3CBea99509d93f8DDDC8C6FB",
            "0xD1220A0cf47c7B9Be7A2E6BA89F429762e7b9aDb",
        ];
        for addr in addresses {
            let parsed1: Address = addr.parse().unwrap();
            let parsed2 = Address::parse_checksummed(addr, None).unwrap();
            assert_eq!(parsed1, parsed2);
            assert_eq!(parsed2.to_checksum(None), addr);
        }
    }

    // https://eips.ethereum.org/EIPS/eip-1191
    #[test]
    fn checksum_chain_id() {
        let eth_mainnet = [
            "0x27b1fdb04752bbc536007a920d24acb045561c26",
            "0x3599689E6292b81B2d85451025146515070129Bb",
            "0x42712D45473476b98452f434e72461577D686318",
            "0x52908400098527886E0F7030069857D2E4169EE7",
            "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed",
            "0x6549f4939460DE12611948b3f82b88C3C8975323",
            "0x66f9664f97F2b50F62D13eA064982f936dE76657",
            "0x8617E340B3D01FA5F11F306F4090FD50E238070D",
            "0x88021160C5C792225E4E5452585947470010289D",
            "0xD1220A0cf47c7B9Be7A2E6BA89F429762e7b9aDb",
            "0xdbF03B407c01E7cD3CBea99509d93f8DDDC8C6FB",
            "0xde709f2102306220921060314715629080e2fb77",
            "0xfB6916095ca1df60bB79Ce92cE3Ea74c37c5d359",
        ];
        let rsk_mainnet = [
            "0x27b1FdB04752BBc536007A920D24ACB045561c26",
            "0x3599689E6292B81B2D85451025146515070129Bb",
            "0x42712D45473476B98452f434E72461577d686318",
            "0x52908400098527886E0F7030069857D2E4169ee7",
            "0x5aaEB6053f3e94c9b9a09f33669435E7ef1bEAeD",
            "0x6549F4939460DE12611948B3F82B88C3C8975323",
            "0x66F9664f97f2B50F62d13EA064982F936de76657",
            "0x8617E340b3D01Fa5f11f306f4090fd50E238070D",
            "0x88021160c5C792225E4E5452585947470010289d",
            "0xD1220A0Cf47c7B9BE7a2e6ba89F429762E7B9adB",
            "0xDBF03B407c01E7CD3cBea99509D93F8Dddc8C6FB",
            "0xDe709F2102306220921060314715629080e2FB77",
            "0xFb6916095cA1Df60bb79ce92cE3EA74c37c5d359",
        ];
        let rsk_testnet = [
            "0x27B1FdB04752BbC536007a920D24acB045561C26",
            "0x3599689e6292b81b2D85451025146515070129Bb",
            "0x42712D45473476B98452F434E72461577D686318",
            "0x52908400098527886E0F7030069857D2e4169EE7",
            "0x5aAeb6053F3e94c9b9A09F33669435E7EF1BEaEd",
            "0x6549f4939460dE12611948b3f82b88C3c8975323",
            "0x66f9664F97F2b50f62d13eA064982F936DE76657",
            "0x8617e340b3D01fa5F11f306F4090Fd50e238070d",
            "0x88021160c5C792225E4E5452585947470010289d",
            "0xd1220a0CF47c7B9Be7A2E6Ba89f429762E7b9adB",
            "0xdbF03B407C01E7cd3cbEa99509D93f8dDDc8C6fB",
            "0xDE709F2102306220921060314715629080e2Fb77",
            "0xFb6916095CA1dF60bb79CE92ce3Ea74C37c5D359",
        ];
        for (addresses, chain_id) in [(eth_mainnet, 1), (rsk_mainnet, 30), (rsk_testnet, 31)] {
            // EIP-1191 test cases treat mainnet as "not adopted"
            let id = if chain_id == 1 { None } else { Some(chain_id) };
            for addr in addresses {
                let parsed1: Address = addr.parse().unwrap();
                let parsed2 = Address::parse_checksummed(addr, id).unwrap();
                assert_eq!(parsed1, parsed2);
                assert_eq!(parsed2.to_checksum(id), addr);
            }
        }
    }

    // https://ethereum.stackexchange.com/questions/760/how-is-the-address-of-an-ethereum-contract-computed
    #[test]
    #[cfg(feature = "rlp")]
    fn create() {
        let from = "0x6ac7ea33f8831ea9dcc53393aaa88b25a785dbf0"
            .parse::<Address>()
            .unwrap();
        for (nonce, expected) in [
            "0xcd234a471b72ba2f1ccf0a70fcaba648a5eecd8d",
            "0x343c43a37d37dff08ae8c4a11544c718abb4fcf8",
            "0xf778b86fa74e846c4f0a1fbd1335fe81c00a0c91",
            "0xfffd933a0bc612844eaf0c6fe3e5b8e9b6c1d19c",
        ]
        .into_iter()
        .enumerate()
        {
            let address = from.create(nonce as u64);
            assert_eq!(address, expected.parse::<Address>().unwrap());
        }
    }

    #[test]
    #[cfg(all(feature = "rlp", feature = "arbitrary"))]
    fn create_correctness() {
        fn create_slow(address: &Address, nonce: u64) -> Address {
            use alloy_rlp::Encodable;

            let mut out = vec![];

            alloy_rlp::Header {
                list: true,
                payload_length: address.length() + nonce.length(),
            }
            .encode(&mut out);
            address.encode(&mut out);
            nonce.encode(&mut out);

            Address::from_word(keccak256(out))
        }

        proptest::proptest!(|(address: Address, nonce: u64)| {
            proptest::prop_assert_eq!(address.create(nonce), create_slow(&address, nonce));
        });
    }

    // https://eips.ethereum.org/EIPS/eip-1014
    #[test]
    fn create2() {
        let tests = [
            (
                "0000000000000000000000000000000000000000",
                "0000000000000000000000000000000000000000000000000000000000000000",
                "00",
                "4D1A2e2bB4F88F0250f26Ffff098B0b30B26BF38",
            ),
            (
                "deadbeef00000000000000000000000000000000",
                "0000000000000000000000000000000000000000000000000000000000000000",
                "00",
                "B928f69Bb1D91Cd65274e3c79d8986362984fDA3",
            ),
            (
                "deadbeef00000000000000000000000000000000",
                "000000000000000000000000feed000000000000000000000000000000000000",
                "00",
                "D04116cDd17beBE565EB2422F2497E06cC1C9833",
            ),
            (
                "0000000000000000000000000000000000000000",
                "0000000000000000000000000000000000000000000000000000000000000000",
                "deadbeef",
                "70f2b2914A2a4b783FaEFb75f459A580616Fcb5e",
            ),
            (
                "00000000000000000000000000000000deadbeef",
                "00000000000000000000000000000000000000000000000000000000cafebabe",
                "deadbeef",
                "60f3f640a8508fC6a86d45DF051962668E1e8AC7",
            ),
            (
                "00000000000000000000000000000000deadbeef",
                "00000000000000000000000000000000000000000000000000000000cafebabe",
                "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
                "1d8bfDC5D46DC4f61D6b6115972536eBE6A8854C",
            ),
            (
                "0000000000000000000000000000000000000000",
                "0000000000000000000000000000000000000000000000000000000000000000",
                "",
                "E33C0C7F7df4809055C3ebA6c09CFe4BaF1BD9e0",
            ),
        ];
        for (from, salt, init_code, expected) in tests {
            let from = from.parse::<Address>().unwrap();

            let salt = hex::decode(salt).unwrap();
            let salt: [u8; 32] = salt.try_into().unwrap();

            let init_code = hex::decode(init_code).unwrap();
            let init_code_hash = keccak256(&init_code);

            let expected = expected.parse::<Address>().unwrap();

            assert_eq!(expected, from.create2(salt, init_code_hash));
            assert_eq!(expected, from.create2_from_code(salt, init_code));
        }
    }
}
