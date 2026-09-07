//! Bounded, duplicate-rejecting canonical JSON with zeroizing string values.
//! It is private to the protected inner codec; errors discard parser details.
use super::{Error, Result};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{
    de::{self, MapAccess, SeqAccess, Visitor},
    Deserialize, Deserializer, Serialize, Serializer,
};
use std::fmt;
use zeroize::Zeroizing;

pub(super) enum Node {
    Null,
    Bool(bool),
    Number(serde_json::Number),
    String(Zeroizing<String>),
    Array(Vec<Node>),
    Object(Vec<(Zeroizing<String>, Node)>),
}
impl<'de> Deserialize<'de> for Node {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Node;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("bounded protected JSON")
            }
            fn visit_unit<E: de::Error>(self) -> std::result::Result<Node, E> {
                Ok(Node::Null)
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> std::result::Result<Node, E> {
                Ok(Node::Bool(v))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> std::result::Result<Node, E> {
                Ok(Node::Number(v.into()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> std::result::Result<Node, E> {
                Ok(Node::Number(v.into()))
            }
            fn visit_f64<E: de::Error>(self, _: f64) -> std::result::Result<Node, E> {
                Err(E::custom("integer required"))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Node, E> {
                Ok(Node::String(Zeroizing::new(v.to_owned())))
            }
            fn visit_string<E: de::Error>(self, v: String) -> std::result::Result<Node, E> {
                Ok(Node::String(Zeroizing::new(v)))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> std::result::Result<Node, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = a.next_element()? {
                    if values.len() >= 512 {
                        return Err(de::Error::custom("array bound"));
                    }
                    values.push(value);
                }
                Ok(Node::Array(values))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> std::result::Result<Node, A::Error> {
                let mut values: Vec<(Zeroizing<String>, Node)> = Vec::new();
                while let Some(key) = a.next_key::<String>()? {
                    let key = Zeroizing::new(key);
                    if values.len() >= 64
                        || key.len() > 128
                        || values.iter().any(|(k, _)| k.as_str() == key.as_str())
                    {
                        return Err(de::Error::custom("object bound or duplicate"));
                    }
                    values.push((key, a.next_value()?));
                }
                values.sort_by(|(a, _), (b, _)| a.as_str().cmp(b.as_str()));
                Ok(Node::Object(values))
            }
        }
        deserializer.deserialize_any(V)
    }
}
impl Serialize for Node {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        match self {
            Self::Null => s.serialize_unit(),
            Self::Bool(v) => s.serialize_bool(*v),
            Self::Number(v) => v.serialize(s),
            Self::String(v) => s.serialize_str(v),
            Self::Array(v) => {
                let mut a = s.serialize_seq(Some(v.len()))?;
                for value in v {
                    a.serialize_element(value)?;
                }
                a.end()
            }
            Self::Object(v) => {
                let mut m = s.serialize_map(Some(v.len()))?;
                for (k, value) in v {
                    m.serialize_entry(k.as_str(), value)?;
                }
                m.end()
            }
        }
    }
}
impl Node {
    pub(super) fn parse(bytes: &[u8], max: usize) -> Result<Self> {
        if bytes.is_empty() || bytes.len() > max || bytes.first() != Some(&b'{') {
            return Err(Error::Protocol);
        }
        let value: Self = serde_json::from_slice(bytes).map_err(|_| Error::Protocol)?;
        if value.bytes()?.as_slice() != bytes {
            return Err(Error::Protocol);
        }
        Ok(value)
    }
    pub(super) fn bytes(&self) -> Result<Zeroizing<Vec<u8>>> {
        Ok(Zeroizing::new(
            serde_json::to_vec(self).map_err(|_| Error::Protocol)?,
        ))
    }
    pub(super) fn keys(&self, expected: &[&str]) -> Result<()> {
        match self {
            Self::Object(v)
                if v.len() == expected.len()
                    && expected
                        .iter()
                        .all(|k| v.iter().any(|(x, _)| x.as_str() == *k)) =>
            {
                Ok(())
            }
            _ => Err(Error::Protocol),
        }
    }
    pub(super) fn field(&self, key: &str) -> Result<&Self> {
        match self {
            Self::Object(v) => v
                .iter()
                .find(|(k, _)| k.as_str() == key)
                .map(|(_, v)| v)
                .ok_or(Error::Protocol),
            _ => Err(Error::Protocol),
        }
    }
    pub(super) fn text(&self) -> Result<&str> {
        match self {
            Self::String(s) => Ok(s),
            _ => Err(Error::Protocol),
        }
    }
    pub(super) fn integer(&self) -> Result<u64> {
        match self {
            Self::Number(n) => n.as_u64().ok_or(Error::Protocol),
            _ => Err(Error::Protocol),
        }
    }
    pub(super) fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }
    pub(super) fn empty_array(&self) -> bool {
        matches!(self,Self::Array(v) if v.is_empty())
    }
}
