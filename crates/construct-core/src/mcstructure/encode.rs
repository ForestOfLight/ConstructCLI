//! Encoding [`Structure`] back to `.mcstructure` bytes.
//!
//! `decode`'s invariants are checked again on the way out, because `merge`
//! builds a `Structure` in memory: a grid disagreeing with its own `size`
//! would write happily and fail to load in-game.
//!
//! Output is not byte-stable — `nbtx` stores compounds in a `HashMap`, so key
//! order varies. Compounds are unordered, so round-trip tests compare decoded
//! values rather than bytes (§12).

use super::decode::{Structure, VOID};
use super::nbt::bad;
use crate::error::Result;
use std::collections::HashMap;

fn int_list(v: &[i32]) -> nbtx::Value {
    nbtx::Value::List(v.iter().copied().map(nbtx::Value::Int).collect())
}

pub fn encode(s: &Structure, what: &str) -> Result<Vec<u8>> {
    if s.size.x < 0 || s.size.y < 0 || s.size.z < 0 {
        return Err(bad(
            what,
            format!(
                "size has a negative dimension: [{}, {}, {}]",
                s.size.x, s.size.y, s.size.z
            ),
        ));
    }
    let volume =
        usize::try_from(s.size.volume()).map_err(|_| bad(what, "size is too large to address"))?;
    for (i, layer) in s.layers.iter().enumerate() {
        if layer.len() != volume {
            return Err(bad(
                what,
                format!(
                    "layer {i} has {} entries but size [{}, {}, {}] needs {volume}",
                    layer.len(),
                    s.size.x,
                    s.size.y,
                    s.size.z
                ),
            ));
        }
    }
    let palette_len =
        i32::try_from(s.palette.len()).map_err(|_| bad(what, "palette is too large"))?;
    for (i, layer) in s.layers.iter().enumerate() {
        if let Some(bad_index) = layer
            .iter()
            .find(|&&b| b != VOID && (b < 0 || b >= palette_len))
        {
            return Err(bad(
                what,
                format!(
                    "layer {i} refers to palette entry {bad_index}, but the palette has {palette_len}"
                ),
            ));
        }
    }
    for &index in s.block_position_data.keys() {
        if index >= volume {
            return Err(bad(
                what,
                format!("block_position_data index {index} is outside the structure"),
            ));
        }
    }

    let block_palette = nbtx::Value::List(
        s.palette
            .iter()
            .map(|b| {
                nbtx::Value::Compound(HashMap::from([
                    ("name".to_string(), nbtx::Value::String(b.name.clone())),
                    ("states".to_string(), b.states.clone()),
                    ("version".to_string(), nbtx::Value::Int(b.version)),
                ]))
            })
            .collect(),
    );

    let block_position_data = nbtx::Value::Compound(
        s.block_position_data
            .iter()
            .map(|(i, v)| (i.to_string(), v.clone()))
            .collect::<HashMap<_, _>>(),
    );

    let default = nbtx::Value::Compound(HashMap::from([
        ("block_palette".to_string(), block_palette),
        ("block_position_data".to_string(), block_position_data),
    ]));

    let structure = nbtx::Value::Compound(HashMap::from([
        (
            "block_indices".to_string(),
            nbtx::Value::List(vec![int_list(&s.layers[0]), int_list(&s.layers[1])]),
        ),
        (
            "entities".to_string(),
            nbtx::Value::List(s.entities.clone()),
        ),
        (
            "palette".to_string(),
            nbtx::Value::Compound(HashMap::from([("default".to_string(), default)])),
        ),
    ]));

    let root = nbtx::Value::Compound(HashMap::from([
        (
            "format_version".to_string(),
            nbtx::Value::Int(s.format_version),
        ),
        (
            "size".to_string(),
            int_list(&[s.size.x, s.size.y, s.size.z]),
        ),
        ("structure".to_string(), structure),
        (
            "structure_world_origin".to_string(),
            int_list(&[s.origin.x, s.origin.y, s.origin.z]),
        ),
    ]));

    nbtx::to_le_bytes(&root).map_err(|e| bad(what, format!("could not write NBT: {e}")))
}
