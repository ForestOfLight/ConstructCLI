//! A builder for `.mcstructure` NBT, so tests can state a fixture's shape
//! instead of hand-assembling bytes.
//!
//! Fixtures are built rather than committed as binaries: the five shapes spec
//! §12 asks for are then readable in the test that uses them, and no opaque
//! blob has to be trusted. One real committed file
//! (`tests/fixtures/construct.mcstructure`) covers what a builder cannot —
//! that the codec agrees with what the game actually writes.

use std::collections::HashMap;

pub fn compound(pairs: Vec<(&str, nbtx::Value)>) -> nbtx::Value {
    nbtx::Value::Compound(
        pairs
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect::<HashMap<_, _>>(),
    )
}

pub fn int_list(v: &[i32]) -> nbtx::Value {
    nbtx::Value::List(v.iter().copied().map(nbtx::Value::Int).collect())
}

/// One entry of `block_palette`.
pub fn block(name: &str) -> nbtx::Value {
    compound(vec![
        ("name", nbtx::Value::String(name.to_string())),
        ("states", compound(vec![])),
        ("version", nbtx::Value::Int(18163713)),
    ])
}

pub struct Build {
    pub size: [i32; 3],
    pub origin: [i32; 3],
    pub layer0: Vec<i32>,
    pub layer1: Vec<i32>,
    pub palette: Vec<nbtx::Value>,
    pub block_position_data: Vec<(String, nbtx::Value)>,
    pub entities: Vec<nbtx::Value>,
    pub format_version: i32,
}

impl Build {
    /// A structure of `size` filled with palette entry 0 on layer 0 and void
    /// on layer 1 — the shape the overwhelming majority of real files have.
    pub fn solid(size: [i32; 3], origin: [i32; 3], name: &str) -> Self {
        let volume = (size[0] * size[1] * size[2]) as usize;
        Self {
            size,
            origin,
            layer0: vec![0; volume],
            layer1: vec![-1; volume],
            palette: vec![block(name)],
            block_position_data: vec![],
            entities: vec![],
            format_version: 1,
        }
    }

    pub fn nbt(&self) -> nbtx::Value {
        let palette_default = compound(vec![
            ("block_palette", nbtx::Value::List(self.palette.clone())),
            (
                "block_position_data",
                nbtx::Value::Compound(
                    self.block_position_data
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect::<HashMap<_, _>>(),
                ),
            ),
        ]);
        let structure = compound(vec![
            (
                "block_indices",
                nbtx::Value::List(vec![int_list(&self.layer0), int_list(&self.layer1)]),
            ),
            ("entities", nbtx::Value::List(self.entities.clone())),
            ("palette", compound(vec![("default", palette_default)])),
        ]);
        compound(vec![
            ("format_version", nbtx::Value::Int(self.format_version)),
            ("size", int_list(&self.size)),
            ("structure", structure),
            ("structure_world_origin", int_list(&self.origin)),
        ])
    }

    pub fn bytes(&self) -> Vec<u8> {
        nbtx::to_le_bytes(&self.nbt()).expect("fixture must serialize")
    }
}
