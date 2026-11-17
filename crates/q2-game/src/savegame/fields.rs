//! `#[serde(with = ...)]` adapters that save callbacks and monster moves by
//! their registered names (C `WriteField1`'s `F_FUNCTION` and `F_MMOVE`).

use serde::de::Error as _;
use serde::ser::Error as _;
use serde::{Deserialize, Deserializer, Serializer};

use super::registry::{by_name, move_by_name, move_name, name_of, Callback};
use crate::entity::MonsterMove;

/// An `Option<callback>` field.
pub mod callback {
    use super::*;

    pub fn serialize<F: Callback, S: Serializer>(f: &Option<F>, s: S) -> Result<S::Ok, S::Error> {
        match f {
            None => s.serialize_none(),
            Some(f) => match name_of(*f) {
                Some(name) => s.serialize_some(name),
                None => Err(S::Error::custom(format!(
                    "unregistered callback at {:#x}: see savegame/registry.rs",
                    f.addr()
                ))),
            },
        }
    }

    pub fn deserialize<'de, F: Callback, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Option<F>, D::Error> {
        let name: Option<&str> = Option::deserialize(d)?;
        name.map(|name| {
            by_name(name).ok_or_else(|| D::Error::custom(format!("unknown callback {name}")))
        })
        .transpose()
    }
}

/// `MonsterInfo::currentmove`.
pub mod monster_move {
    use super::*;

    pub fn serialize<S: Serializer>(mv: &Option<MonsterMove>, s: S) -> Result<S::Ok, S::Error> {
        match mv {
            None => s.serialize_none(),
            Some(mv) => match move_name(mv) {
                Some(name) => s.serialize_some(name),
                None => Err(S::Error::custom(format!(
                    "unregistered monster move (frames {}..={})",
                    mv.firstframe, mv.lastframe
                ))),
            },
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<MonsterMove>, D::Error> {
        let name: Option<&str> = Option::deserialize(d)?;
        name.map(|name| {
            move_by_name(name)
                .cloned()
                .ok_or_else(|| D::Error::custom(format!("unknown monster move {name}")))
        })
        .transpose()
    }
}

/// An array longer than serde's built-in 32 (`pers.inventory`).
pub mod array {
    use super::*;
    use serde::Serialize;

    pub fn serialize<T: Serialize, S: Serializer, const N: usize>(
        a: &[T; N],
        s: S,
    ) -> Result<S::Ok, S::Error> {
        s.collect_seq(a)
    }

    pub fn deserialize<'de, T: Deserialize<'de>, D: Deserializer<'de>, const N: usize>(
        d: D,
    ) -> Result<[T; N], D::Error> {
        let items = Vec::<T>::deserialize(d)?;
        let len = items.len();
        items
            .try_into()
            .map_err(|_| D::Error::invalid_length(len, &"the saved array length"))
    }
}
