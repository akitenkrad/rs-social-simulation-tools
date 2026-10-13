//! Typed reading of scenario tables.
//!
//! [`ParamReader`] reads one TOML table, records every key it reads, and
//! reports the keys it never read as unknown in [`ParamReader::finish`]. The
//! value-level functions check one value each; they are shared by the reader
//! and by the built-in rule constructors, which receive the parameters as a
//! [`socsim_config::Params`].
//!
//! No function here falls back to a default value when a value has the wrong
//! type: every mismatch is a [`GameError::Config`] whose message starts with
//! the path of the offending key.

use std::collections::BTreeSet;

use crate::GameError;

/// `2^64`, the first value above the range of `u64`.
const TWO_POW_64: f64 = 18_446_744_073_709_551_616.0;

/// A configuration error located at `path`.
pub(crate) fn config_error(path: &str, reason: impl std::fmt::Display) -> GameError {
    GameError::Config(format!("{path}: {reason}"))
}

/// Joins a table path and a key: `world.game` and `kind` give
/// `world.game.kind`.
pub(crate) fn join(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_owned()
    } else {
        format!("{path}.{key}")
    }
}

fn type_error(path: &str, expected: &str, value: &toml::Value) -> GameError {
    config_error(
        path,
        format!("expected {expected}, found {}", value.type_str()),
    )
}

/// Reads a real number: a TOML integer or a finite TOML float.
pub(crate) fn real_value(path: &str, value: &toml::Value) -> Result<f64, GameError> {
    match value {
        toml::Value::Integer(integer) => Ok(*integer as f64),
        toml::Value::Float(float) if float.is_finite() => Ok(*float),
        toml::Value::Float(float) => Err(config_error(
            path,
            format!("expected a finite number, found {float}"),
        )),
        other => Err(type_error(path, "a number", other)),
    }
}

/// Reads a count: a non-negative integer, written as a TOML integer or as a
/// finite TOML float with no fractional part, that fits in `T`.
pub(crate) fn count_value<T>(path: &str, value: &toml::Value) -> Result<T, GameError>
where
    T: TryFrom<u64>,
{
    let wide = match value {
        toml::Value::Integer(integer) => u64::try_from(*integer).map_err(|_| {
            config_error(
                path,
                format!("expected a non-negative integer, found {integer}"),
            )
        })?,
        toml::Value::Float(float) => {
            if !float.is_finite() {
                return Err(config_error(
                    path,
                    format!("expected a non-negative integer, found {float}"),
                ));
            }
            if float.fract() != 0.0 {
                return Err(config_error(
                    path,
                    format!("expected an integer, found {float}"),
                ));
            }
            if *float < 0.0 {
                return Err(config_error(
                    path,
                    format!("expected a non-negative integer, found {float}"),
                ));
            }
            if *float >= TWO_POW_64 {
                return Err(config_error(
                    path,
                    format!("{float} is out of range for {}", std::any::type_name::<T>()),
                ));
            }
            *float as u64
        }
        other => return Err(type_error(path, "a non-negative integer", other)),
    };
    T::try_from(wide).map_err(|_| {
        config_error(
            path,
            format!("{wide} is out of range for {}", std::any::type_name::<T>()),
        )
    })
}

/// Reads a boolean.
pub(crate) fn flag_value(path: &str, value: &toml::Value) -> Result<bool, GameError> {
    value
        .as_bool()
        .ok_or_else(|| type_error(path, "a boolean", value))
}

/// Reads a string.
pub(crate) fn text_value<'a>(path: &str, value: &'a toml::Value) -> Result<&'a str, GameError> {
    value
        .as_str()
        .ok_or_else(|| type_error(path, "a string", value))
}

/// Whether `label` is a non-empty ASCII `[A-Za-z0-9_-]+` action label.
pub(crate) fn is_label(label: &str) -> bool {
    !label.is_empty()
        && label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

/// Reads an action label: a string of ASCII letters, digits, `_`, and `-`.
pub(crate) fn label_value<'a>(path: &str, value: &'a toml::Value) -> Result<&'a str, GameError> {
    let label = text_value(path, value)?;
    if !is_label(label) {
        return Err(config_error(
            path,
            format!("{label:?} is not an action label (ASCII letters, digits, '_' and '-')"),
        ));
    }
    Ok(label)
}

/// Reads one of the strings in `choices`.
pub(crate) fn choice_value<'a>(
    path: &str,
    value: &'a toml::Value,
    choices: &[&str],
) -> Result<&'a str, GameError> {
    let text = text_value(path, value)?;
    if choices.contains(&text) {
        Ok(text)
    } else {
        Err(config_error(
            path,
            format!("expected one of {}, found {text:?}", quoted(choices)),
        ))
    }
}

/// Reads an array, checking every element with `element`.
pub(crate) fn array_value<'a, T>(
    path: &str,
    value: &'a toml::Value,
    mut element: impl FnMut(&str, &'a toml::Value) -> Result<T, GameError>,
) -> Result<Vec<T>, GameError> {
    let array = value
        .as_array()
        .ok_or_else(|| type_error(path, "an array", value))?;
    array
        .iter()
        .enumerate()
        .map(|(index, item)| element(&format!("{path}[{index}]"), item))
        .collect()
}

/// Reads an array of real numbers.
pub(crate) fn reals_value(path: &str, value: &toml::Value) -> Result<Vec<f64>, GameError> {
    array_value(path, value, real_value)
}

/// Renders `["a", "b"]` as `"a", "b"`.
pub(crate) fn quoted(choices: &[&str]) -> String {
    choices
        .iter()
        .map(|choice| format!("{choice:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Reads the keys of one TOML table and reports the keys it did not read.
pub(crate) struct ParamReader<'a> {
    table: &'a toml::Table,
    path: String,
    used: BTreeSet<String>,
}

impl<'a> ParamReader<'a> {
    /// A reader of `table`, whose keys are reported as `path.<key>`.
    pub(crate) fn new(table: &'a toml::Table, path: impl Into<String>) -> Self {
        Self {
            table,
            path: path.into(),
            used: BTreeSet::new(),
        }
    }

    /// The path of `key` in the table.
    pub(crate) fn key_path(&self, key: &str) -> String {
        join(&self.path, key)
    }

    /// An error located at `key`.
    pub(crate) fn error(&self, key: &str, reason: impl std::fmt::Display) -> GameError {
        config_error(&self.key_path(key), reason)
    }

    /// An error located at the table itself.
    pub(crate) fn table_error(&self, reason: impl std::fmt::Display) -> GameError {
        config_error(&self.path, reason)
    }

    /// The table being read.
    pub(crate) fn table(&self) -> &'a toml::Table {
        self.table
    }

    /// Whether the table has `key`. Does not mark the key as read.
    pub(crate) fn has(&self, key: &str) -> bool {
        self.table.contains_key(key)
    }

    /// Marks `keys` as known without reading them.
    pub(crate) fn accept<'k>(&mut self, keys: impl IntoIterator<Item = &'k str>) {
        self.used.extend(keys.into_iter().map(str::to_owned));
    }

    /// The raw value of `key`, marking it as read.
    pub(crate) fn value(&mut self, key: &str) -> Option<&'a toml::Value> {
        self.used.insert(key.to_owned());
        self.table.get(key)
    }

    fn read<T>(
        &mut self,
        key: &str,
        read: impl FnOnce(&str, &'a toml::Value) -> Result<T, GameError>,
    ) -> Result<Option<T>, GameError> {
        match self.value(key) {
            Some(value) => read(&self.key_path(key), value).map(Some),
            None => Ok(None),
        }
    }

    /// Fails with "required" when `value` is `None`.
    pub(crate) fn required<T>(&self, key: &str, value: Option<T>) -> Result<T, GameError> {
        value.ok_or_else(|| self.error(key, "this key is required"))
    }

    /// An optional real number (see [`real_value`]).
    pub(crate) fn real(&mut self, key: &str) -> Result<Option<f64>, GameError> {
        self.read(key, real_value)
    }

    /// A required real number.
    pub(crate) fn require_real(&mut self, key: &str) -> Result<f64, GameError> {
        let value = self.real(key)?;
        self.required(key, value)
    }

    /// An optional count of type `u32` (see [`count_value`]).
    pub(crate) fn count_u32(&mut self, key: &str) -> Result<Option<u32>, GameError> {
        self.read(key, count_value::<u32>)
    }

    /// An optional count of type `usize` (see [`count_value`]).
    pub(crate) fn count_usize(&mut self, key: &str) -> Result<Option<usize>, GameError> {
        self.read(key, count_value::<usize>)
    }

    /// A required count of type `usize`.
    pub(crate) fn require_count_usize(&mut self, key: &str) -> Result<usize, GameError> {
        let value = self.count_usize(key)?;
        self.required(key, value)
    }

    /// An optional boolean.
    pub(crate) fn flag(&mut self, key: &str) -> Result<Option<bool>, GameError> {
        self.read(key, flag_value)
    }

    /// An optional string.
    pub(crate) fn text(&mut self, key: &str) -> Result<Option<&'a str>, GameError> {
        self.read(key, text_value)
    }

    /// An optional action label (see [`label_value`]).
    pub(crate) fn label(&mut self, key: &str) -> Result<Option<&'a str>, GameError> {
        self.read(key, label_value)
    }

    /// An optional string that must be one of `choices`.
    pub(crate) fn choice(
        &mut self,
        key: &str,
        choices: &[&str],
    ) -> Result<Option<&'a str>, GameError> {
        self.read(key, |path, value| choice_value(path, value, choices))
    }

    /// A required string that must be one of `choices`.
    pub(crate) fn require_choice(
        &mut self,
        key: &str,
        choices: &[&str],
    ) -> Result<&'a str, GameError> {
        let value = self.choice(key, choices)?;
        self.required(key, value)
    }

    /// An optional array of real numbers.
    pub(crate) fn reals(&mut self, key: &str) -> Result<Option<Vec<f64>>, GameError> {
        self.read(key, reals_value)
    }

    /// An optional nested table, read by its own reader.
    pub(crate) fn subtable(&mut self, key: &str) -> Result<Option<ParamReader<'a>>, GameError> {
        let path = self.key_path(key);
        match self.value(key) {
            Some(toml::Value::Table(table)) => Ok(Some(ParamReader::new(table, path))),
            Some(other) => Err(type_error(&path, "a table", other)),
            None => Ok(None),
        }
    }

    /// Reports the first key that was never read as unknown.
    pub(crate) fn finish(self) -> Result<(), GameError> {
        let unknown: Vec<&str> = self
            .table
            .keys()
            .map(String::as_str)
            .filter(|key| !self.used.contains(*key))
            .collect();
        match unknown.first() {
            None => Ok(()),
            Some(key) => {
                let known: Vec<&str> = self.used.iter().map(String::as_str).collect();
                Err(self.error(
                    key,
                    format!("unknown key (known keys: {})", known.join(", ")),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(text: &str) -> toml::Value {
        let table: toml::Table = toml::from_str(&format!("v = {text}")).unwrap();
        table["v"].clone()
    }

    #[test]
    fn counts_accept_integral_values_in_range() {
        assert_eq!(count_value::<u64>("p", &value("16")).unwrap(), 16);
        assert_eq!(count_value::<u64>("p", &value("16.0")).unwrap(), 16);
        assert_eq!(
            count_value::<u32>("p", &value("4294967295")).unwrap(),
            u32::MAX
        );
        for bad in ["-1", "1.5", "1e30", "nan", "inf", "\"16\"", "true", "-2.0"] {
            assert!(count_value::<u64>("p", &value(bad)).is_err(), "{bad}");
        }
        assert!(count_value::<u32>("p", &value("4294967296")).is_err());
    }

    #[test]
    fn reals_accept_integers_and_finite_floats() {
        assert_eq!(real_value("p", &value("2")).unwrap(), 2.0);
        assert_eq!(real_value("p", &value("2.5")).unwrap(), 2.5);
        for bad in ["\"x\"", "true", "nan", "inf", "-inf"] {
            assert!(real_value("p", &value(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn unknown_keys_are_reported_with_their_path() {
        let table: toml::Table = toml::from_str("kind = \"x\"\ntemptaton = 2").unwrap();
        let mut reader = ParamReader::new(&table, "world.game");
        reader.text("kind").unwrap();
        let message = reader.finish().unwrap_err().to_string();
        assert!(message.contains("world.game.temptaton"), "{message}");
    }
}
