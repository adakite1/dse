/// There's a bug in serde where serializing with the attribute #[serde(flatten)] works,
///  but for any format other than JSON the deserializer crashes when reading back those same files.
/// While it remains unfixed, a workaround was created thanks to @tobz1000 on Github and shared here: https://github.com/RReverser/serde-xml-rs/issues/137#issuecomment-695341295
/// Although we can just not flatten the prgi chunk's headers into the entries themselves, the two other chunks, wavi and kgrp are both already
///  flat by design (the header is not stored in a separate struct key like in prgi), so I thought it would be helpful to also
///  flatten prgi :)

/// Workaround for a bug with deserialising flattened structs with serde-xml-rs:
/// https://github.com/RReverser/serde-xml-rs/issues/137
/// To apply this workaround, add this attribute to any primitive field which is either in a
/// flattened struct, or is being deserialised in a flattened struct via a transparent newtype:
/// `#[deserialize_with = "flattened_xml_attr"]`

use fmt::Display;
use serde::{Deserialize, Deserializer};
use std::fmt;

// Inspired by https://docs.rs/serde-aux/0.6.1/serde_aux/field_attributes/fn.deserialize_number_from_string.html
pub fn flattened_xml_attr<'de, D: Deserializer<'de>, T: FromXmlStr + Deserialize<'de>>(
    deserializer: D,
) -> Result<T, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum TypeOrString<T> {
        Ty(T),
        String(String),
    }

    match TypeOrString::<T>::deserialize(deserializer)? {
        TypeOrString::Ty(t) => Ok(t),
        TypeOrString::String(s) => T::from_str(&s).map_err(serde::de::Error::custom),
    }
}

/// Trait to define on types which we need to deserialize from XML within a flattened struct, for
/// which the `std::str::FromStr` is absent/unsuitable. This should mirror the behaviour of
/// serde-xml-rs for Serde data model types.
pub trait FromXmlStr: Sized {
    type Error: Display;
    fn from_str(s: &str) -> Result<Self, Self::Error>;
    fn deserialize_from_type<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error>;
}

macro_rules! impl_from_xml_str_as_from_str {
    ($($t:ty)*) => {
        $(
            impl FromXmlStr for $t {
                type Error = <$t as std::str::FromStr>::Err;
                fn from_str(s: &str) -> Result<Self, Self::Error> {
                    s.parse()
                }

                fn deserialize_from_type<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                    <$t>::deserialize(deserializer)
                }
            }
        )*
    };
}

impl_from_xml_str_as_from_str! {
    usize u8 u16 u32 u64 u128
    isize i8 i16 i32 i64 i128
    f32 f64 char
}

/// Can parse from "1"/"0" as well as "true"/"false".
impl FromXmlStr for bool {
    type Error = String;

    fn from_str(s: &str) -> Result<Self, Self::Error> {
        match s {
            "true" | "1" => Ok(true),
            "false" | "0" => Ok(false),
            s => Err(format!("\"{}\" is not a valid bool", s)),
        }
    }

    fn deserialize_from_type<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        bool::deserialize(deserializer)
    }
}