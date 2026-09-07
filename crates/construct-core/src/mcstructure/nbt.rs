use crate::error::{CoreError, Result};

pub(crate) fn bad(what: &str, reason: impl Into<String>) -> CoreError {
    CoreError::BadStructureFile {
        what: what.to_string(),
        reason: reason.into(),
    }
}

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

pub(crate) fn as_compound<'a>(
    v: &'a nbtx::Value,
    name: &str,
    what: &str,
) -> Result<&'a std::collections::HashMap<String, nbtx::Value>> {
    match v {
        nbtx::Value::Compound(m) => Ok(m),
        _ => Err(bad(what, format!("field {name:?} is not a compound"))),
    }
}

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

pub(crate) fn as_int_vec(v: &nbtx::Value, name: &str, what: &str) -> Result<Vec<i32>> {
    as_list(v, name, what)?
        .iter()
        .map(|e| as_int(e, name, what))
        .collect()
}
