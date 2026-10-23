//! Reject ignored unknown fields recursively without changing additive public events.
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use std::fmt;
pub(super) fn decode<'a, T: Deserialize<'a>>(text: &'a str) -> Result<T, serde_json::Error> {
    let mut d = serde_json::Deserializer::from_str(text);
    let result = T::deserialize(Strict(&mut d))?;
    d.end()?;
    Ok(result)
}
struct Strict<D>(D);
impl<'de, D: Deserializer<'de>> Deserializer<'de> for Strict<D> {
    type Error = D::Error;
    fn deserialize_any<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Self::Error> {
        self.0.deserialize_any(Wrap(v))
    }
    fn deserialize_option<V: Visitor<'de>>(self, v: V) -> Result<V::Value, Self::Error> {
        self.0.deserialize_option(Wrap(v))
    }
    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        n: &'static str,
        v: V,
    ) -> Result<V::Value, Self::Error> {
        self.0.deserialize_newtype_struct(n, Wrap(v))
    }
    fn deserialize_ignored_any<V: Visitor<'de>>(self, _: V) -> Result<V::Value, Self::Error> {
        Err(de::Error::custom("Unknown report record field"))
    }
    serde::forward_to_deserialize_any! {bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char str string bytes byte_buf unit unit_struct seq tuple tuple_struct map struct enum identifier}
}
struct Wrap<V>(V);
macro_rules! scalar {($($method:ident($ty:ty)),*)=>{$(fn $method<E:de::Error>(self,v:$ty)->Result<Self::Value,E> {self.0.$method(v)})*};}
impl<'de, V: Visitor<'de>> Visitor<'de> for Wrap<V> {
    type Value = V::Value;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.expecting(f)
    }
    scalar! {visit_bool(bool),visit_i8(i8),visit_i16(i16),visit_i32(i32),visit_i64(i64),visit_u8(u8),visit_u16(u16),visit_u32(u32),visit_u64(u64),visit_f32(f32),visit_f64(f64),visit_char(char),visit_string(String),visit_byte_buf(Vec<u8>)}
    fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
        if v.len() > 3 * eml_core::MAX_COLLECTED_PATH_BYTES {
            return Err(E::custom("Report string bound"));
        }
        self.0.visit_str(v)
    }
    fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<Self::Value, E> {
        if v.len() > 3 * eml_core::MAX_COLLECTED_PATH_BYTES {
            return Err(E::custom("Report string bound"));
        }
        self.0.visit_borrowed_str(v)
    }
    fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Self::Value, E> {
        self.0.visit_bytes(v)
    }
    fn visit_borrowed_bytes<E: de::Error>(self, v: &'de [u8]) -> Result<Self::Value, E> {
        self.0.visit_borrowed_bytes(v)
    }
    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        self.0.visit_none()
    }
    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        self.0.visit_unit()
    }
    fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        self.0.visit_some(Strict(d))
    }
    fn visit_newtype_struct<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        self.0.visit_newtype_struct(Strict(d))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, a: A) -> Result<Self::Value, A::Error> {
        self.0.visit_seq(Access(a, 0))
    }
    fn visit_map<A: MapAccess<'de>>(self, a: A) -> Result<Self::Value, A::Error> {
        self.0.visit_map(Access(a, 0))
    }
}
struct Seed<S>(S);
impl<'de, S: DeserializeSeed<'de>> DeserializeSeed<'de> for Seed<S> {
    type Value = S::Value;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        self.0.deserialize(Strict(d))
    }
}
struct Access<A>(A, usize);
impl<'de, A: MapAccess<'de>> MapAccess<'de> for Access<A> {
    type Error = A::Error;
    fn next_key_seed<S: DeserializeSeed<'de>>(
        &mut self,
        s: S,
    ) -> Result<Option<S::Value>, Self::Error> {
        self.0.next_key_seed(Seed(s))
    }
    fn next_value_seed<S: DeserializeSeed<'de>>(&mut self, s: S) -> Result<S::Value, Self::Error> {
        self.0.next_value_seed(Seed(s))
    }
    fn size_hint(&self) -> Option<usize> {
        self.0.size_hint()
    }
}
impl<'de, A: SeqAccess<'de>> SeqAccess<'de> for Access<A> {
    type Error = A::Error;
    fn next_element_seed<S: DeserializeSeed<'de>>(
        &mut self,
        s: S,
    ) -> Result<Option<S::Value>, Self::Error> {
        if self.1 == eml_core::MAX_MESSAGE_PARTS {
            return match self.0.next_element::<de::IgnoredAny>()? {
                None => Ok(None),
                Some(_) => Err(de::Error::custom("Report sequence bound")),
            };
        }
        let value = self.0.next_element_seed(Seed(s))?;
        if value.is_some() {
            self.1 += 1;
        }
        Ok(value)
    }
    fn size_hint(&self) -> Option<usize> {
        self.0.size_hint()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sequence_limit_is_checked_before_decoding_extra_element_and_equality_is_valid() {
        let mut text = String::from("[");
        text.push_str(&vec!["0"; eml_core::MAX_MESSAGE_PARTS].join(","));
        text.push(']');
        assert_eq!(
            decode::<Vec<u8>>(&text).unwrap().len(),
            eml_core::MAX_MESSAGE_PARTS
        );
        text.pop();
        text.push_str(",0]");
        assert!(decode::<Vec<u8>>(&text).is_err());
    }
}
