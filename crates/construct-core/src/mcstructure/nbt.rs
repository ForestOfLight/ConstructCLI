//! Typed field access over `nbtx::Value`.
//!
//! Decoding is field extraction with a specific error per missing or
//! mistyped field. Written as nested `match` arms it becomes unreadable and
//! the errors collapse into one vague message, so the extraction lives here
//! and `decode.rs` reads as a list of fields.

use crate::error::{CoreError, Result};

pub(crate) fn bad(what: &str, reason: impl Into<String>) -> CoreError {
    CoreError::BadStructureFile {
        what: what.to_string(),
        reason: reason.into(),
    }
}

/// A named field of a compound, or an error naming it.
pub(crate) fn field<'a>(v: &'a nbtx::Value, name: &str, what: &str) -> Result<&'a nbtx::Value> {
    match v {
        nbtx::Value::Compound(m) => m
            .get(name)
            .ok_or_else(|| bad(what, format!("required field {name:?} is missing"))),
        _ => Err(bad(
            what,
            format!("expected a compound to read {name:?} from"),
        )),
    }
}

pub(crate) fn as_int(v: &nbtx::Value, name: &str, what: &str) -> Result<i32> {
    match v {
        nbtx::Value::Int(i) => Ok(*i),
        _ => Err(bad(what, format!("field {name:?} is not an int"))),
    }
}

pub(crate) fn as_list<'a>(
    v: &'a nbtx::Value,
    name: &str,
    what: &str,
) -> Result<&'a Vec<nbtx::Value>> {
    match v {
        nbtx::Value::List(l) => Ok(l),
        _ => Err(bad(what, format!("field {name:?} is not a list"))),
    }
}

/// A list of exactly three ints — `size` and `structure_world_origin`.
pub(crate) fn as_triple(v: &nbtx::Value, name: &str, what: &str) -> Result<[i32; 3]> {
    let list = as_list(v, name, what)?;
    if list.len() != 3 {
        return Err(bad(
            what,
            format!(
                "field {name:?} needs exactly 3 values, found {}",
                list.len()
            ),
        ));
    }
    Ok([
        as_int(&list[0], name, what)?,
        as_int(&list[1], name, what)?,
        as_int(&list[2], name, what)?,
    ])
}

/// A list of ints. The reference doc notes the game treats non-int entries as
/// `0`; this refuses instead, because a file that vague is more likely damaged
/// than intentional and silently rewriting it as air would hide that.
pub(crate) fn as_int_vec(v: &nbtx::Value, name: &str, what: &str) -> Result<Vec<i32>> {
    as_list(v, name, what)?
        .iter()
        .map(|e| as_int(e, name, what))
        .collect()
}
